//! Windows のアプリ（WinUI 3）。左にサイドバー（TreeView: Workspace・Branches・Remotes・Tags・あるときだけ
//! Stashes と Submodules）、右に History（SwapChainPanel に Direct2D で描く）。macOS 版の窓の骨格に合わせる。
//!
//! 部品は windows-reactor で宣言的に作る。reactor が出していない入力（ホイール・キー・押した位置）は、History を
//! 包む Grid の生の WinUI のオブジェクト（`request_native_element`）に、`amonhen-win-xaml` の型で直に付ける。
//! WinUI のハンドラーは `Send` でなければならないので何も持たず、UI のスレッドの `HISTORY` から描き手を取る。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use amonhen_core::RefsSnapshot;
use amonhen_core::Repository;
use amonhen_model::RefEntry;
use amonhen_win_xaml as xaml;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows_core_reactor::{IInspectable, Interface as _, Ref};
use windows_reactor::*;

use crate::render::{ROW_HEIGHT, Surface};
use crate::sidebar::{self, BranchGroups};
use crate::{Args, Session};

thread_local! {
    /// 根の部品に渡す引数（`Component::Input` は比べられる値でなければならないので、ここに置く）。
    static ARGS: RefCell<Option<(Args, Instant)>> = const { RefCell::new(None) };
    /// History の描き手。
    static HISTORY: RefCell<Option<Rc<RefCell<HistoryView>>>> = const { RefCell::new(None) };
    /// History に付けたイベント（持っている間だけ届く）。
    static REVOKERS: RefCell<Vec<windows_core_reactor::EventRevoker>> =
        const { RefCell::new(Vec::new()) };
}

/// ホイールの 1 目盛り（120）で進む行の数（Windows の既定の 3 行）。
const WHEEL_ROWS: f32 = 3.0;

pub fn run(args: Args, started: Instant) -> Result<(), String> {
    ARGS.with(|a| *a.borrow_mut() = Some((args, started)));
    App::run_component::<Root>(()).map_err(|e| format!("WinUI を始められない: {e}"))
}

fn with_history(f: impl FnOnce(&mut HistoryView)) {
    if let Some(view) = HISTORY.with(|h| h.borrow().clone()) {
        f(&mut view.borrow_mut());
    }
}

struct Root {
    panel: ElementRef<SwapChainPanel>,
    area: ElementRef<Grid>,
    refs: Option<Arc<RefsSnapshot>>,
    /// ローカルブランチの分け方（コアの `branchGroups`）。
    groups: Option<BranchGroups>,
    error: Option<String>,
}

enum Message {
    Refs(Arc<RefsSnapshot>, Arc<Repository>),
    Groups(Option<BranchGroups>),
    /// サイドバーの項目を選んだ（項目のキー）。
    Invoked(String),
    Failed(String),
}

impl Component for Root {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        Self {
            panel: ElementRef::new(),
            area: ElementRef::new(),
            refs: None,
            groups: None,
            error: None,
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Refs(refs, repo) => {
                self.refs = Some(refs);
                // ブランチの分け方は ref が変わるたびに読み直す（数十 ms かかるので裏で）。
                context.spawn_background(move |_| {
                    let reply = repo.run_json(r#"{"op":"branchGroups"}"#);
                    let reply = serde_json::from_str(&reply).unwrap_or_default();
                    Message::Groups(BranchGroups::from_reply(&reply))
                });
            }
            Message::Groups(groups) => {
                if groups.is_some() {
                    self.groups = groups;
                }
            }
            Message::Failed(e) => self.error = Some(e),
            Message::Invoked(key) => {
                if let Some(entry) = self.entry(&key)
                    && let Some(target) = entry.target
                {
                    with_history(|h| h.select_commit(&target));
                }
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        let (args, _) = ARGS.with(|a| a.borrow().clone()).expect("args");
        let name = std::fs::canonicalize(&args.repo)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| args.repo.display().to_string());
        context.window_title(name);
        let (panel, area, sender) = (self.panel.clone(), self.area.clone(), context.sender());
        context.use_effect("history", (), move || {
            let (args, started) = ARGS.with(|a| a.borrow().clone()).expect("args");
            let session = match Session::new(&args, "app", started, Arc::new(|| {})) {
                Ok(s) => s,
                Err(e) => {
                    sender.send(Message::Failed(e));
                    return None;
                }
            };
            let view = Rc::new(RefCell::new(HistoryView {
                session,
                panel: panel.clone(),
                area,
                options: args.options,
                attached: false,
                input_requested: false,
                sender,
                refs_sent: None,
            }));
            HISTORY.with(|h| *h.borrow_mut() = Some(view.clone()));
            let observation = panel.observe_surface(|event| {
                with_history(|h| {
                    if let Err(e) = h.on_event(event) {
                        h.sender.send(Message::Failed(e));
                    }
                })
            });
            Some(Box::new(move || {
                drop(observation);
                REVOKERS.with(|r| r.borrow_mut().clear());
                HISTORY.with(|h| h.borrow_mut().take());
            }))
        });

        let sidebar = TreeView::new()
            .on_item_invoked(context.callback(Message::Invoked))
            .grid_column(0)
            .nodes(sidebar::nodes(
                self.refs.as_ref().map(|r| &r.refs),
                self.groups.as_ref(),
            ));
        let history: View = match &self.error {
            Some(e) => TextBlock::new().text(e.clone()).margin(16.0).into(),
            None => SwapChainPanel::new().element_ref(&self.panel).into(),
        };
        Grid::new()
            .columns([GridLength::Pixel(260.0), GridLength::Star(1.0)])
            .children((
                sidebar,
                Grid::new()
                    .background(Brush::Solid(Color::transparent()))
                    .element_ref(&self.area)
                    .grid_column(1)
                    .children((history,)),
            ))
    }
}

impl Root {
    /// サイドバーの項目のキーが指す ref。
    fn entry(&self, key: &str) -> Option<&RefEntry> {
        let (kind, name) = sidebar::parse_ref_key(key)?;
        self.refs
            .as_ref()?
            .refs
            .entries
            .iter()
            .find(|e| e.kind as u8 == kind && e.name == name)
    }
}

struct HistoryView {
    session: Session,
    panel: ElementRef<SwapChainPanel>,
    area: ElementRef<Grid>,
    options: crate::render::Options,
    attached: bool,
    input_requested: bool,
    sender: LocalSender<Message>,
    /// サイドバーに送った ref（同じものは送り直さない）。
    refs_sent: Option<Arc<RefsSnapshot>>,
}

impl HistoryView {
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
                    let unknown = unsafe { windows_core_reactor::IUnknown::from_raw(raw) };
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

    /// 届いたものを取り込み、描くものがあれば描く。
    fn render(&mut self) -> Result<(), String> {
        if !self.input_requested {
            self.input_requested = self.area.request_native_element(|result| match result {
                Ok(element) => {
                    if let Err(e) = attach_input(&element) {
                        eprintln!("History に入力を付けられない: {e}");
                    }
                }
                Err(e) => eprintln!("History の部品を取れない: {e:?}"),
            });
        }
        let s = &mut self.session;
        s.poll()?;
        if let (Some(refs), Some(repo)) = (&s.data.refs, s.data.repo())
            && !self
                .refs_sent
                .as_ref()
                .is_some_and(|sent| Arc::ptr_eq(sent, refs))
        {
            self.refs_sent = Some(refs.clone());
            self.sender.send(Message::Refs(refs.clone(), repo));
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
            std::process::exit(0);
        }
        Ok(())
    }

    fn select_commit(&mut self, id: &amonhen_model::Oid) {
        if let Some(row) = self.session.data.row_of(id) {
            self.session.reveal(row);
        }
    }
}

/// History を包む Grid に、押す・ホイール・キーを付け、キーを受けられるようにする。
fn attach_input(element: &IInspectable) -> windows_core_reactor::Result<()> {
    let ui: xaml::UIElement = element.cast()?;
    ui.SetIsTabStop(true)?;
    let mut revokers = Vec::new();

    let target = ui.clone();
    revokers.push(
        ui.PointerPressed(move |_, args: Ref<xaml::PointerRoutedEventArgs>| {
            let Some(args) = args.as_ref() else { return };
            let Ok(point) = args.GetCurrentPoint(&target) else {
                return;
            };
            let (Ok(position), Ok(properties)) = (point.Position(), point.Properties()) else {
                return;
            };
            if !properties.IsLeftButtonPressed().unwrap_or(false) {
                return;
            }
            let _ = target.Focus(xaml::FocusState::Pointer);
            with_history(|h| {
                if let Some(row) = h.session.row_at(position.Y) {
                    h.session.select(row);
                }
            });
            let _ = args.SetHandled(true);
        })?,
    );

    revokers.push(
        ui.PointerWheelChanged(|_, args: Ref<xaml::PointerRoutedEventArgs>| {
            let Some(args) = args.as_ref() else { return };
            let Ok(properties) = args
                .GetCurrentPoint(None::<&xaml::UIElement>)
                .and_then(|p| p.Properties())
            else {
                return;
            };
            if properties.IsHorizontalMouseWheel().unwrap_or(false) {
                return;
            }
            let delta = properties.MouseWheelDelta().unwrap_or(0) as f32;
            with_history(|h| {
                h.session
                    .scroll_by(-delta / 120.0 * WHEEL_ROWS * ROW_HEIGHT)
            });
            let _ = args.SetHandled(true);
        })?,
    );

    revokers.push(ui.KeyDown(|_, args: Ref<xaml::KeyRoutedEventArgs>| {
        let Some(args) = args.as_ref() else { return };
        let Ok(key) = args.Key() else { return };
        let handled = std::cell::Cell::new(true);
        with_history(|h| {
            let s = &mut h.session;
            match key {
                xaml::VirtualKey::Up => s.move_selection(-1),
                xaml::VirtualKey::Down => s.move_selection(1),
                xaml::VirtualKey::PageUp => s.move_selection(-s.page_rows()),
                xaml::VirtualKey::PageDown => s.move_selection(s.page_rows()),
                xaml::VirtualKey::Home => s.move_selection(-(s.data.row_count() as i64)),
                xaml::VirtualKey::End => s.move_selection(s.data.row_count() as i64),
                _ => handled.set(false),
            }
        });
        if handled.get() {
            let _ = args.SetHandled(true);
        }
    })?);

    REVOKERS.with(|r| r.borrow_mut().extend(revokers));
    Ok(())
}
