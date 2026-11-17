//! WinUI 3（windows-reactor）のウィンドウ。左にサイドバー（WinUI の部品）、右の SwapChainPanel に同じ History を描く。
//!
//! 描くのは SwapChainPanel の `Rendering`（XAML の合成のフレームごと）。swap chain は Composition 用に作り、
//! XAML の合成の木に入る。Independent Flip（MPO）に上がるかどうかは PresentMon で見る。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::Threading::{GetCurrentThreadId, WaitForSingleObjectEx};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CallWindowProcW, EnumChildWindows, EnumThreadWindows, GA_ROOT, GWLP_WNDPROC,
    GetAncestor, GetClassNameW, GetWindowThreadProcessId, MSG, PM_REMOVE, SetWindowLongPtrW,
    SetWindowsHookExW, WH_GETMESSAGE, WM_APP, WM_LBUTTONDOWN, WM_POINTERDOWN, WNDPROC,
};
use windows::core::BOOL;
use windows_reactor::*;

use crate::render::Surface;
use crate::{Args, Session};

thread_local! {
    /// SwapChainPanel を包む Border が受け取ったクリックの時刻（QPC）。次の `Rendering` で取り込む。
    static CLICKS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };

    /// `--draw-on-input` のとき、クリックを受けたその場で描くための描き手。
    static HOST: RefCell<Option<std::rc::Weak<RefCell<Host>>>> = const { RefCell::new(None) };

    /// `--raw-input`: 差し替えた子ウィンドウの元の手続き。
    static SUBCLASSED: RefCell<HashMap<isize, isize>> = RefCell::new(HashMap::new());

    /// `--raw-input`: 子ウィンドウで受けた時刻と、XAML（Border）が受けた時刻（比べるため）。
    static RAW_DOWNS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    static XAML_DOWNS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };

    static RAW: Cell<bool> = const { Cell::new(false) };

    /// `--raw-input`: 差し替えた窓に来た入力のメッセージの数（窓、メッセージ）。最初の XAML のクリックで書く。
    static SEEN: RefCell<HashMap<(isize, u32), usize>> = RefCell::new(HashMap::new());
    static SEEN_LINE: RefCell<Option<String>> = const { RefCell::new(None) };
    static SEEN_TOTAL: Cell<usize> = const { Cell::new(0) };
    static SEEN_REPORTED: Cell<bool> = const { Cell::new(false) };

    /// 後で書く行（描く途中では書けないので、次の `render` で書く）。
    static PENDING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Raw Input のスレッドが受けた、左ボタンを押した時刻（OS から届いた時刻に近い）。
static OS_DOWNS: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
/// WinUI の窓（Raw Input のスレッドから知らせる先）。
static TOP: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
const WM_RAW_PRESS: u32 = WM_APP + 1;
/// `--child-hwnd`: Raw Input のスレッドは時刻を取るだけにする。
static CHILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {

    /// 根の部品に渡す引数（`Component::Input` は比べられる値でなければならないので、ここに置く）。
    static ARGS: RefCell<Option<(Args, Instant)>> = const { RefCell::new(None) };
}

pub fn run(args: Args, started: Instant) -> Result<(), String> {
    ARGS.with(|a| *a.borrow_mut() = Some((args, started)));
    App::run_component::<Root>(()).map_err(|e| format!("WinUI を始められない: {e}"))
}

struct Root {
    panel: ElementRef<SwapChainPanel>,
}

impl Component for Root {
    type Input = ();
    type Message = ();

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        Self {
            panel: ElementRef::new(),
        }
    }

    fn update(&mut self, _message: (), _context: &ComponentContext<Self>) {}

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("Amon Hen (WinUI 3)");
        let panel = self.panel.clone();
        context.use_effect("surface", (), move || {
            let (args, started) = ARGS.with(|a| a.borrow().clone()).expect("args");
            // Rendering が毎フレーム来るので、裏のスレッドから起こす必要はない。
            let session = match Session::new(&args, "winui", started, Arc::new(|| {})) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            };
            let host = Rc::new(RefCell::new(Host {
                session,
                panel: panel.clone(),
                options: args.options,
                attached: false,
            }));
            RAW.set(args.raw_input);
            if args.raw_input {
                install_hook();
                CHILD.store(args.child_hwnd, std::sync::atomic::Ordering::Release);
                spawn_raw_input();
            }
            if args.draw_on_input {
                HOST.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&host)));
            }
            let h = host.clone();
            let observation = panel.observe_surface(move |event| {
                if let Err(e) = h.borrow_mut().on_event(event) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            });
            Some(Box::new(move || {
                drop(observation);
                drop(host);
            }))
        });
        let sidebar = StackPanel::new()
            .spacing(6.0)
            .margin(12.0)
            .grid_column(0)
            .children((
                TextBlock::new().text("Workspace").font_size(12.0),
                TextBlock::new().text("Working Copy"),
                TextBlock::new().text("History"),
                TextBlock::new().text("Branches").font_size(12.0),
                TextBlock::new().text("main"),
            ));
        Grid::new()
            .columns([GridLength::Pixel(220.0), GridLength::Star(1.0)])
            .children((
                sidebar,
                Border::new()
                    .background(Brush::Solid(Color::transparent()))
                    .on_pointer_pressed(|_: PointerEventInfo| {
                        let at = crate::measure::now();
                        if RAW.get() {
                            // 子ウィンドウで受けて描いた後。比べるために時刻だけ取る。
                            XAML_DOWNS.with(|x| x.borrow_mut().push(at));
                            let os = OS_DOWNS.lock().unwrap().last().copied();
                            let handled = RAW_DOWNS.with(|r| r.borrow().last().copied());
                            PENDING.with(|p| {
                                p.borrow_mut().push(format!(
                                    "press os={} handled={} xaml={at}",
                                    os.unwrap_or(0),
                                    handled.unwrap_or(0)
                                ))
                            });
                            if !SEEN_REPORTED.replace(true) {
                                let mut seen: Vec<String> = SEEN.with(|m| {
                                    m.borrow()
                                        .iter()
                                        .map(|(&(h, msg), n)| {
                                            format!("{}:{msg:#06x}x{n}", class_name(HWND(h as _)))
                                        })
                                        .collect()
                                });
                                seen.sort();
                                println!("input messages: {}", seen.join(" "));
                                eprintln!("input messages: {}", seen.join(" "));
                                SEEN_LINE.with(|l| *l.borrow_mut() = Some(seen.join(" ")));
                            }
                            return;
                        }
                        CLICKS.with(|c| c.borrow_mut().push(at));
                        // 次の Rendering を待たずに描く（`--draw-on-input`）。
                        if let Some(host) =
                            HOST.with(|h| h.borrow().as_ref().and_then(|w| w.upgrade()))
                            && let Err(e) = host.borrow_mut().render()
                        {
                            eprintln!("{e}");
                            std::process::exit(1);
                        }
                    })
                    .grid_column(1)
                    .content(SwapChainPanel::new().element_ref(&self.panel)),
            ))
    }
}

struct Host {
    session: Session,
    panel: ElementRef<SwapChainPanel>,
    options: crate::render::Options,
    attached: bool,
}

impl Host {
    fn on_event(&mut self, event: SwapChainPanelEvent) -> Result<(), String> {
        match event {
            SwapChainPanelEvent::Metrics {
                width,
                height,
                scale_x,
                ..
            } => {
                let s = &mut self.session;
                s.scale = scale_x;
                let (w, h) = (
                    (width * scale_x as f64).round() as u32,
                    (height * scale_x as f64).round() as u32,
                );
                match s.surface.as_mut() {
                    Some(surface) => surface
                        .resize(&s.renderer, w, h)
                        .map_err(|e| e.to_string())?,
                    None => {
                        s.surface = Some(
                            Surface::for_composition(&s.renderer, w, h, self.options)
                                .map_err(|e| format!("swap chain を作れない: {e}"))?,
                        );
                    }
                }
                let surface = s.surface.as_ref().expect("surface");
                surface.set_scale(scale_x).map_err(|e| e.to_string())?;
                if !self.attached {
                    self.attached = true;
                    use windows::core::Interface as _;
                    let raw = surface
                        .chain
                        .cast::<windows::core::IUnknown>()
                        .map_err(|e| e.to_string())?
                        .into_raw();
                    // SAFETY: 同じ COM のオブジェクトを、reactor が使う windows-core の型で持ち直すだけ。
                    let unknown = unsafe {
                        <windows_core_reactor::IUnknown as windows_core_reactor::Interface>::from_raw(
                            raw,
                        )
                    };
                    let accepted = self.panel.request_set_swap_chain(unknown, |result| {
                        if let Err(e) = result {
                            eprintln!("SwapChainPanel に swap chain をつなげない: {e:?}");
                        }
                    });
                    if !accepted {
                        self.attached = false;
                    }
                }
                s.invalidate();
            }
            SwapChainPanelEvent::Rendering => self.render()?,
        }
        Ok(())
    }

    /// 届いたものとクリックを取り込み、描くものがあれば描く。
    fn render(&mut self) -> Result<(), String> {
        if RAW.get() {
            subclass_windows(&self.session);
        }
        let s = &mut self.session;
        s.poll()?;
        for at in CLICKS.with(|c| std::mem::take(&mut *c.borrow_mut())) {
            s.clicked(at);
        }
        if s.awaiting_display() && s.poll_input() {
            finish(s);
        }
        if !s.needs_frame() {
            return Ok(());
        }
        // 前のフレームが出るのを待つ（最大 1 フレーム。UI のスレッドなので長くは待たない）。
        if let Some(h) = s.surface.as_ref().and_then(|s| s.waitable)
            && unsafe { WaitForSingleObjectEx(h, 20, false) } != WAIT_OBJECT_0
        {
            return Ok(());
        }
        if s.frame()? {
            finish(s);
        }
        Ok(())
    }
}

/// 終わる。`--raw-input` なら、子ウィンドウで受けてから XAML が受けるまでの差も書く。
fn finish(s: &Session) -> ! {
    if RAW.get() {
        let raw = RAW_DOWNS.with(|r| r.borrow().clone());
        let xaml = XAML_DOWNS.with(|x| x.borrow().clone());
        let freq = {
            let mut f = 0;
            unsafe {
                let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut f);
            }
            f as f64
        };
        let mut gaps: Vec<f64> = xaml
            .iter()
            .filter_map(|&x| {
                raw.iter()
                    .filter(|&&r| r <= x)
                    .max()
                    .map(|&r| (x - r) as f64 * 1000.0 / freq)
            })
            .collect();
        gaps.sort_by(f64::total_cmp);
        if !gaps.is_empty() {
            s.report(&format!(
                "raw window to XAML pointer: p50 {:.2} / p95 {:.2} / max {:.2} ms ({} of {} raw)",
                gaps[(gaps.len() - 1) / 2],
                gaps[((gaps.len() - 1) as f64 * 0.95).round() as usize],
                gaps[gaps.len() - 1],
                gaps.len(),
                raw.len()
            ));
        } else {
            s.report(&format!("raw {} / XAML {} clicks", raw.len(), xaml.len()));
        }
    }
    std::process::exit(0);
}

/// この UI スレッドの窓と、その子ウィンドウをすべて差し替える（後から作られるものもあるので毎回見る）。
fn subclass_windows(s: &Session) {
    unsafe extern "system" fn child(hwnd: HWND, _: LPARAM) -> BOOL {
        subclass(hwnd);
        true.into()
    }
    unsafe extern "system" fn top(hwnd: HWND, _: LPARAM) -> BOOL {
        subclass(hwnd);
        unsafe {
            let _ = EnumChildWindows(Some(hwnd), Some(child), LPARAM(0));
        }
        true.into()
    }
    for line in PENDING.with(|p| std::mem::take(&mut *p.borrow_mut())) {
        s.report(&line);
    }
    if let Some(line) = SEEN_LINE.with(|l| l.borrow_mut().take()) {
        s.report(&format!("first XAML press, input messages: {line}"));
    }
    let total: usize = SEEN.with(|m| m.borrow().values().sum());
    if total != SEEN_TOTAL.replace(total) {
        let mut seen: Vec<String> = SEEN.with(|m| {
            m.borrow()
                .iter()
                .map(|(&(h, msg), n)| format!("{}:{msg:#06x}x{n}", class_name(HWND(h as _))))
                .collect()
        });
        seen.sort();
        s.report(&format!("input messages: {}", seen.join(" ")));
    }
    let before = SUBCLASSED.with(|m| m.borrow().len());
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(top), LPARAM(0));
    }
    let added: Vec<String> = SUBCLASSED.with(|m| {
        let m = m.borrow();
        if m.len() == before {
            return Vec::new();
        }
        m.keys()
            .map(|&h| {
                let tid = unsafe { GetWindowThreadProcessId(HWND(h as _), None) };
                format!("{}@{tid}", class_name(HWND(h as _)))
            })
            .collect()
    });
    if !added.is_empty() {
        s.report(&format!(
            "subclassed windows (UI thread {}): {}",
            unsafe { GetCurrentThreadId() },
            added.join(", ")
        ));
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn subclass(hwnd: HWND) {
    let key = hwnd.0 as isize;
    if SUBCLASSED.with(|m| m.borrow().contains_key(&key)) {
        return;
    }
    let old = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, raw_proc as *const () as isize) };
    if old != 0 {
        if class_name(hwnd) == "WinUIDesktopWin32WindowClass" {
            TOP.store(key, std::sync::atomic::Ordering::Release);
            if CHILD.load(std::sync::atomic::Ordering::Acquire) {
                create_child(hwnd);
            }
        }
        SUBCLASSED.with(|m| m.borrow_mut().insert(key, old));
    }
}

/// UI のスレッドがメッセージを取り出した時点で受ける（WinUI 3 は押すメッセージを窓の手続きに配らず、
/// 取り出した後の `ContentPreTranslateMessage` で処理するので、子ウィンドウの差し替えでは見えない）。
fn install_hook() {
    unsafe {
        if let Err(e) =
            SetWindowsHookExW(WH_GETMESSAGE, Some(hook_proc), None, GetCurrentThreadId())
        {
            eprintln!("フックを入れられない: {e}");
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 == PM_REMOVE.0 as usize {
        let msg = unsafe { &*(lparam.0 as *const MSG) };
        if (0x0200..=0x02ff).contains(&msg.message) {
            SEEN.with(|m| {
                *m.borrow_mut()
                    .entry((msg.hwnd.0 as isize, msg.message))
                    .or_default() += 1
            });
        }
        if (msg.message == WM_POINTERDOWN || msg.message == WM_LBUTTONDOWN)
            && in_panel(msg.hwnd, msg.message, msg.lParam)
        {
            pressed(crate::measure::now());
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn pressed(at: i64) {
    // WM_POINTERDOWN の後に互換の WM_LBUTTONDOWN が来ることがあるので、続けて来たものは 1 回にする。
    let repeated = RAW_DOWNS.with(|r| r.borrow().last().is_some_and(|&last| (at - last) < 50_000));
    if repeated {
        return;
    }
    RAW_DOWNS.with(|r| r.borrow_mut().push(at));
    CLICKS.with(|c| c.borrow_mut().push(at));
    if let Some(host) = HOST.with(|h| h.borrow().as_ref().and_then(|w| w.upgrade()))
        && let Ok(mut host) = host.try_borrow_mut()
        && let Err(e) = host.render()
    {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

unsafe extern "system" fn raw_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_RAW_PRESS {
        pressed(lparam.0 as i64);
        return LRESULT(0);
    }
    if (0x0200..=0x02ff).contains(&msg) || (0x0100..=0x0109).contains(&msg) || msg == 0x00ff {
        SEEN.with(|m| *m.borrow_mut().entry((hwnd.0 as isize, msg)).or_default() += 1);
    }
    if (msg == WM_POINTERDOWN || msg == WM_LBUTTONDOWN) && in_panel(hwnd, msg, lparam) {
        let at = crate::measure::now();
        // WM_POINTERDOWN の後に互換の WM_LBUTTONDOWN が来ることがあるので、続けて来たものは 1 回にする。
        let repeated = RAW_DOWNS.with(|r| {
            r.borrow().last().is_some_and(|&last| (at - last) < 50_000) // QPC は 10 MHz（5 ms）
        });
        if !repeated {
            RAW_DOWNS.with(|r| r.borrow_mut().push(at));
            CLICKS.with(|c| c.borrow_mut().push(at));
            if let Some(host) = HOST.with(|h| h.borrow().as_ref().and_then(|w| w.upgrade()))
                && let Ok(mut host) = host.try_borrow_mut()
                && let Err(e) = host.render()
            {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    let old = SUBCLASSED.with(|m| m.borrow().get(&(hwnd.0 as isize)).copied());
    match old {
        Some(old) => unsafe {
            let proc: WNDPROC = std::mem::transmute::<isize, WNDPROC>(old);
            CallWindowProcW(proc, hwnd, msg, wparam, lparam)
        },
        None => LRESULT(0),
    }
}

/// 押した所が SwapChainPanel の側（サイドバーの右）か。
fn in_panel(hwnd: HWND, msg: u32, lparam: LPARAM) -> bool {
    let mut p = POINT {
        x: (lparam.0 & 0xffff) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xffff) as i16 as i32,
    };
    unsafe {
        // WM_POINTERDOWN は画面の座標、WM_LBUTTONDOWN はその窓の座標。
        if msg == WM_LBUTTONDOWN {
            let _ = ClientToScreen(hwnd, &mut p);
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        let _ = ScreenToClient(root, &mut p);
        let dpi = GetDpiForWindow(root).max(96);
        p.x as f32 >= 220.0 * dpi as f32 / 96.0
    }
}

/// 別のスレッドで Raw Input を受ける。左ボタンが SwapChainPanel の上で押されたら、時刻を記録して WinUI の窓に
/// 知らせる（UI のスレッドが空いていれば、XAML が配るのを待たずに描ける）。
fn spawn_raw_input() {
    use windows::Win32::UI::Input::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
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
                let top = TOP.load(std::sync::atomic::Ordering::Acquire);
                let mut p = POINT::default();
                if top != 0 && unsafe { GetCursorPos(&mut p) }.is_ok() {
                    let top = HWND(top as _);
                    let inside = unsafe {
                        let _ = ScreenToClient(top, &mut p);
                        let mut r = windows::Win32::Foundation::RECT::default();
                        let _ = GetClientRect(top, &mut r);
                        let dpi = GetDpiForWindow(top).max(96);
                        p.x as f32 >= 220.0 * dpi as f32 / 96.0
                            && p.x < r.right
                            && p.y >= 0
                            && p.y < r.bottom
                    };
                    if inside {
                        OS_DOWNS.lock().unwrap().push(at);
                        if !CHILD.load(std::sync::atomic::Ordering::Acquire) {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(top),
                                    WM_RAW_PRESS,
                                    WPARAM(0),
                                    LPARAM(at as isize),
                                );
                            }
                        }
                    }
                }
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    std::thread::spawn(|| unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("AmonHenRawInput"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("AmonHenRawInput"),
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
        ) else {
            eprintln!("Raw Input の窓を作れない");
            return;
        };
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 2,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        if let Err(e) =
            RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
        {
            eprintln!("Raw Input を受けられない: {e}");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

/// SwapChainPanel の上に普通の子ウィンドウを重ねる（押したメッセージを普通に受ける）。描いたものは隠れるので、
/// 届くまでの時間を比べるためだけのもの。
fn create_child(top: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_LBUTTONDOWN || msg == WM_POINTERDOWN {
            pressed(crate::measure::now());
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: w!("AmonHenChild"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let mut r = windows::Win32::Foundation::RECT::default();
        let _ = GetClientRect(top, &mut r);
        let left = (220.0 * GetDpiForWindow(top).max(96) as f32 / 96.0) as i32;
        match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("AmonHenChild"),
            w!(""),
            WS_CHILD | WS_VISIBLE,
            left,
            0,
            r.right - left,
            r.bottom,
            Some(top),
            None,
            None,
            None,
        ) {
            Ok(child) => {
                PENDING.with(|p| p.borrow_mut().push("child window created".into()));
                let _ = SetWindowPos(child, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            }
            Err(e) => PENDING.with(|p| p.borrow_mut().push(format!("child window failed: {e}"))),
        }
    }
}
