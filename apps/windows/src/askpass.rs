//! git と ssh が資格情報（HTTPS のユーザー名とパスワード、SSH の鍵のパスフレーズ、初めてつなぐホストの鍵の確認）を
//! 尋ねる先（GTK4 版の `askpass.rs`、macOS 版の `amonhen-askpass`）。アプリは端末なしで git を動かすので、尋ねられると
//! 失敗する。起動のときに、この実行ファイルを `AMONHEN_ASKPASS` にし、コアが git を動かすときに `GIT_ASKPASS`・
//! `SSH_ASKPASS` にする。git・ssh はプロンプトだけを引数にして呼ぶので、`GIT_ASKPASS`（か `SSH_ASKPASS`）がこの
//! 実行ファイルを指しているときの 1 つだけの引数をプロンプトとみなす。小さな窓で尋ね、答えを標準出力に出す。
//! キャンセルなら 1 で終わる（git は認証に失敗したことにする）。

use std::cell::RefCell;
use std::io::Write;
use std::path::Path;

use amonhen_model::tr;
use amonhen_win_xaml as xaml;
use windows_core_reactor::{IInspectable, Interface as _, Ref};
use windows_reactor::*;

/// `AMONHEN_ASKPASS` をこの実行ファイルにする（もう決まっていれば、そのまま）。ほかのスレッドを動かす前に呼ぶ。
pub fn install() {
    if std::env::var_os("AMONHEN_ASKPASS").is_some() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        // まだほかのスレッドを動かしていない（起動の初め）ので、環境を変えてよい。
        unsafe { std::env::set_var("AMONHEN_ASKPASS", exe) };
    }
}

/// git・ssh から尋ねられて呼ばれたなら、そのプロンプト。
pub fn prompt() -> Option<String> {
    let mut args = std::env::args().skip(1);
    let prompt = args.next()?;
    if args.next().is_some() || prompt.starts_with("--") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let same = |var: &str| std::env::var_os(var).is_some_and(|v| same_file(Path::new(&v), &exe));
    (same("GIT_ASKPASS") || same("SSH_ASKPASS")).then_some(prompt)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// パスワードとパスフレーズは伏せる。ユーザー名と、ホストの鍵の確認は見せる（GTK4 版と同じ）。
fn hidden(prompt: &str) -> bool {
    !(prompt.to_lowercase().contains("username") || prompt.contains("(yes/no"))
}

thread_local! {
    /// 打っている答え。
    static ANSWER: RefCell<String> = const { RefCell::new(String::new()) };
}

/// 答えを出して終わる（`None` ならキャンセル）。
fn finish(answer: Option<String>) -> ! {
    match answer {
        Some(text) => {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{text}");
            let _ = out.flush();
            std::process::exit(0)
        }
        None => std::process::exit(1),
    }
}

/// 尋ねる窓を出す（答えるかキャンセルするまで戻らない）。
pub fn run(prompt: String) -> ! {
    // 窓ができたら、小さくして画面の中ほどへ。
    std::thread::spawn(place_window);
    let _ = App::run_component::<Askpass>(prompt);
    finish(None)
}

struct Askpass {
    field: ElementRef<Grid>,
}

#[derive(Clone)]
enum Message {
    Text(String),
    Ok,
    Cancel,
}

impl Component for Askpass {
    type Input = String;
    type Message = Message;

    fn create(_input: &String, _context: &ComponentContext<Self>) -> Self {
        Self {
            field: ElementRef::new(),
        }
    }

    fn update(&mut self, message: Message, _context: &ComponentContext<Self>) {
        match message {
            Message::Text(text) => ANSWER.with(|a| *a.borrow_mut() = text),
            Message::Ok => finish(Some(ANSWER.with(|a| a.borrow().clone()))),
            Message::Cancel => finish(None),
        }
    }

    fn view(&self, prompt: &String, context: &mut ViewContext<Self>) -> View {
        context.window_title("Amon Hen");
        // 欄に Enter（答える）と Esc（キャンセル）を付け、フォーカスを入れる。
        let field = self.field.clone();
        context.use_effect("field", (), move || {
            let _ = field.request_native_element(|result| {
                if let Ok(element) = result
                    && let Err(e) = attach_keys(&element)
                {
                    eprintln!("欄にキーを付けられない: {e}");
                }
            });
            None
        });
        // 打った値を渡し直す（渡さないと、描き直すたびに欄が空に戻る）。
        let answer = ANSWER.with(|a| a.borrow().clone());
        let input: View = if hidden(prompt) {
            PasswordBox::new()
                .password(answer)
                .on_password_changed(context.callback(Message::Text))
                .into()
        } else {
            TextBox::new()
                .text(answer)
                .on_text_changed(context.callback(Message::Text))
                .into()
        };
        StackPanel::new()
            .spacing(12.0)
            .margin(Thickness::new(20.0, 18.0, 20.0, 18.0))
            .children((
                TextBlock::new()
                    .text(prompt.trim().to_owned())
                    .text_wrapping(TextWrapping::Wrap),
                Grid::new().element_ref(&self.field).children((input,)),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .horizontal_alignment(HorizontalAlignment::Right)
                    .children((
                        Button::new()
                            .on_click(context.callback(|()| Message::Cancel))
                            .content(TextBlock::new().text(tr!("Cancel"))),
                        Button::new()
                            .style(ButtonStyle::Accent)
                            .on_click(context.callback(|()| Message::Ok))
                            .content(TextBlock::new().text("OK")),
                    )),
            ))
    }
}

/// 欄の Enter で答え、Esc でキャンセルする。欄の中の入力の部品にフォーカスを入れる。
fn attach_keys(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    let revoker = ui.PreviewKeyDown(|_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        match args.Key() {
            Ok(xaml::VirtualKey::Enter) => finish(Some(ANSWER.with(|a| a.borrow().clone()))),
            Ok(xaml::VirtualKey::Escape) => finish(None),
            _ => {}
        }
    })?;
    // 窓が閉じるまで外さない（この窓のプロセスは答えたら終わる）。
    std::mem::forget(revoker);
    if let Ok(parent) = element.cast::<xaml::DependencyObject>()
        && let Ok(child) = xaml::VisualTreeHelper::GetChild(&parent, 0)
        && let Ok(control) = child.cast::<xaml::UIElement>()
    {
        let _ = control.Focus(xaml::FocusState::Programmatic);
    }
    Ok(())
}

/// このプロセスの窓を探し、尋ねる窓の大きさにして画面の中ほどへ動かす（2 秒ほどであきらめる）。
fn place_window() {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetSystemMetrics, GetWindowThreadProcessId, IsWindowVisible, SM_CXSCREEN,
        SM_CYSCREEN, SWP_NOZORDER, SetForegroundWindow, SetWindowPos,
    };
    unsafe extern "system" fn found(window: HWND, out: LPARAM) -> windows::core::BOOL {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid == std::process::id() && unsafe { IsWindowVisible(window) }.as_bool() {
            unsafe { *(out.0 as *mut HWND) = window };
            return false.into();
        }
        true.into()
    }
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut window = HWND::default();
        let _ = unsafe { EnumWindows(Some(found), LPARAM(&mut window as *mut HWND as isize)) };
        if window.is_invalid() {
            continue;
        }
        let (w, h) = (560, 260);
        let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        unsafe {
            let _ = SetWindowPos(window, None, (sw - w) / 2, (sh - h) / 3, w, h, SWP_NOZORDER);
            let _ = SetForegroundWindow(window);
        }
        return;
    }
}
