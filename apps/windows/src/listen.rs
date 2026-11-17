//! `--listen <file>`: 別のプロセスで Raw Input を受け、左ボタンを押した時刻（QPC）を 1 行ずつ書く。実のマウスで
//! 測るときの基準（OS が入力を受けた時刻に近い）。試作の窓と同じ機械の QPC なので、そのまま引ける。

use std::io::Write;
use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

thread_local! {
    static OUT: std::cell::RefCell<Option<std::fs::File>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        let at = crate::measure::now();
        let mut raw = RAWINPUT::default();
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let n = unsafe {
            GetRawInputData(
                HRAWINPUT(lparam.0 as _),
                RID_INPUT,
                Some(&mut raw as *mut _ as *mut _),
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        if n != u32::MAX
            && raw.header.dwType == RIM_TYPEMOUSE.0
            && unsafe { raw.data.mouse.Anonymous.Anonymous.usButtonFlags } as u32
                & RI_MOUSE_LEFT_BUTTON_DOWN
                != 0
        {
            OUT.with(|o| {
                if let Some(f) = o.borrow_mut().as_mut() {
                    let _ = writeln!(f, "{at}");
                    let _ = f.flush();
                }
            });
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn run(out: PathBuf) -> Result<(), String> {
    let file = std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    OUT.with(|o| *o.borrow_mut() = Some(file));
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("AmonHenListen"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("AmonHenListen"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .map_err(|e| e.to_string())?;
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 2,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            .map_err(|e| e.to_string())?;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
