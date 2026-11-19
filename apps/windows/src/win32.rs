//! 素の Win32 のウィンドウ（HWND）に swap chain を直接つなぐ。DWM を通さない Independent Flip に最も乗りやすい形。
//!
//! メッセージループは自分で回す。描くものがあるときは swap chain の待てる印（前のフレームが出た）とメッセージを
//! 一緒に待ち、印が立ったら描く。なければメッセージだけを待つ（裏のスレッドは `WM_APP` で起こす）。

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

use crate::render::{Surface, row_height};
use crate::{Args, Session};

enum Input {
    Resize(u32, u32),
    Dpi(u32),
    Wheel(i16),
    Key(VIRTUAL_KEY),
    /// クリックを受け取った時刻（QPC）。
    Click(i64),
}

thread_local! {
    /// ウィンドウプロシージャから受けた入力。ループで取り出す（プロシージャの中で状態を触らない）。
    static INPUTS: RefCell<Vec<Input>> = const { RefCell::new(Vec::new()) };
}

fn push(input: Input) {
    INPUTS.with(|i| i.borrow_mut().push(input));
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let (w, h) = (lparam.0 as u32 & 0xffff, (lparam.0 as u32 >> 16) & 0xffff);
            push(Input::Resize(w, h));
            LRESULT(0)
        }
        WM_DPICHANGED => {
            push(Input::Dpi((wparam.0 as u32 >> 16) & 0xffff));
            // SAFETY: WM_DPICHANGED の lparam は新しい位置の RECT を指す。
            let r = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            push(Input::Wheel((wparam.0 >> 16) as u16 as i16));
            LRESULT(0)
        }
        WM_KEYDOWN => {
            push(Input::Key(VIRTUAL_KEY(wparam.0 as u16)));
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            push(Input::Click(crate::measure::now()));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }
        WM_APP => LRESULT(0),
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn run(args: Args, started: Instant) -> Result<(), String> {
    let hwnd = create_window().map_err(|e| format!("ウィンドウを作れない: {e}"))?;
    let handle = hwnd.0 as usize;
    let wake = Arc::new(move || unsafe {
        let _ = PostMessageW(Some(HWND(handle as _)), WM_APP, WPARAM(0), LPARAM(0));
    });
    let mut session = Session::new(&args, "win32", started, wake)?;
    session.scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.map_err(|e| e.to_string())?;
    let surface = Surface::for_hwnd(
        &session.renderer,
        hwnd,
        (rect.right - rect.left) as u32,
        (rect.bottom - rect.top) as u32,
        args.options,
    )
    .map_err(|e| format!("swap chain を作れない: {e}"))?;
    let waitable = surface.waitable;
    session.surface = Some(surface);
    INPUTS.with(|i| i.borrow_mut().clear());

    // 待てる印が立っている（前のフレームが出た）か。印がなければいつでも描いてよく、Present が垂直同期を待つ。
    let mut ready = waitable.is_none();
    let mut msg = MSG::default();
    loop {
        let wants = session.needs_frame();
        let handles: Vec<HANDLE> = match waitable {
            Some(h) if wants && !ready => vec![h],
            _ => Vec::new(),
        };
        // 画面に出るのを待っている入力があれば、こまめに起きて読む。
        let timeout = if wants && ready {
            0
        } else if session.awaiting_display() {
            2
        } else {
            INFINITE
        };
        let r = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&handles), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if !handles.is_empty() && r == WAIT_OBJECT_0 {
            ready = true;
        }
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return Ok(());
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for input in INPUTS.with(|i| std::mem::take(&mut *i.borrow_mut())) {
            match input {
                Input::Resize(w, h) => {
                    let renderer = &session.renderer;
                    if let Some(s) = session.surface.as_mut() {
                        s.resize(renderer, w, h).map_err(|e| e.to_string())?;
                    }
                    session.invalidate();
                }
                Input::Dpi(dpi) => {
                    session.scale = dpi as f32 / 96.0;
                    session.invalidate();
                }
                Input::Click(at) => session.clicked(at),
                Input::Wheel(delta) => {
                    session.scroll_by(-(delta as f32) / 120.0 * 3.0 * row_height())
                }
                Input::Key(key) => match key {
                    VK_DOWN => session.scroll_by(row_height()),
                    VK_UP => session.scroll_by(-row_height()),
                    VK_NEXT => session.scroll_by(session.page()),
                    VK_PRIOR => session.scroll_by(-session.page()),
                    VK_HOME => session.scroll_to(0.0),
                    VK_END => session.scroll_to(f32::MAX),
                    VK_ESCAPE => unsafe {
                        let _ = DestroyWindow(hwnd);
                    },
                    _ => {}
                },
            }
        }
        session.poll()?;
        if session.awaiting_display() && session.poll_input() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        if session.needs_frame() && ready {
            ready = waitable.is_none();
            if session.frame()? {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }
}

fn create_window() -> windows::core::Result<HWND> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("AmonHenPrototype"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("AmonHenPrototype"),
            w!("Amon Hen (Win32)"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            800,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        // 1280×800 DIP にする（作った時点ではまだ 96 DPI の大きさ）。
        let scale = GetDpiForWindow(hwnd) as f32 / 96.0;
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            (1280.0 * scale) as i32,
            (800.0 * scale) as i32,
            SWP_NOMOVE | SWP_NOZORDER,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
        Ok(hwnd)
    }
}
