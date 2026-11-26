//! Windows の FE（`docs/design.md` の 19）。窓と部品は WinUI 3（windows-reactor、`app.rs`）、History は
//! Direct2D / DirectWrite で描く。`--host win32|winui` は、窓の作りを比べた試作（起動の速さと画面に出るまでの
//! 遅れを測る）。
//!
//! `amonhen-win <repo> [--host app|win32|winui] [--measure-scroll [--scroll-rows <n>] | --measure-input <n>]
//! [--draw-on-input] [--raw-input] [--child-hwnd] [--keep-presenting] [--no-waitable] [--sync <n>]
//! [--tearing] [--profile] [--out <file>]`、`amonhen-win --listen <file>`（実のマウスの基準、`listen.rs`）

mod activity;
mod allfiles;
mod app;
mod askpass;
mod backdrop;
mod blame;
mod branchswitcher;
mod data;
mod drawing;
mod editor;
mod failure;
mod filehistory;
mod files;
mod forecast;
mod github;
mod historyedit;
mod icons;
mod imagediff;
mod listen;
mod markdownfind;
mod markdownview;
mod measure;
mod menus;
mod merge;
mod merge_several;
mod palette;
mod patch_files;
mod pathpicker;
mod prefs;
mod preview;
mod programs;
mod rangediff;
mod reflog;
mod remote;
mod render;
mod reposettings;
mod repositories;
mod sequence;
mod session;
mod settingsview;
mod sidebar;
mod signature;
mod sshhosts;
mod stashes;
mod tabs;
mod templates;
mod theme;
mod timezone;
mod tip;
mod win32;
mod winui;
mod workcopy;

use std::path::PathBuf;
use std::time::Instant;

use data::Data;
use measure::{InputLatency, Measure};
use render::{Options, Renderer, Surface, row_height};

#[derive(Clone)]
pub struct Args {
    pub repo: PathBuf,
    pub host: String,
    pub measure: Option<f32>,
    pub measure_input: Option<usize>,
    /// WinUI 3: クリックを受けたら次の `Rendering` を待たずに描く。
    pub draw_on_input: bool,
    /// WinUI 3: クリックを XAML より前に、子ウィンドウ（HWND）のメッセージで受ける（`--draw-on-input` も兼ねる）。
    pub raw_input: bool,
    /// 変わらなくても毎フレーム描いて present し続ける（Independent Flip から落ちないように。GPUI と同じ出し方）。
    pub keep_presenting: bool,
    /// WinUI 3: SwapChainPanel の上に普通の子ウィンドウ（ほぼ透明）を重ね、クリックをそこで受ける。
    pub child_hwnd: bool,
    pub options: Options,
    pub out: Option<PathBuf>,
    /// 開いたら Repositories の画面を出す（File > New Window で開いた窓）。
    pub repositories: bool,
    /// リポジトリを引数で渡された。
    pub repo_given: bool,
    /// 窓をこの位置・大きさ（画面のピクセル。x, y, 幅, 高さ）に出す（書き込む先の worktree を替えた窓）。
    pub place: Option<[i32; 4]>,
    /// 開いたら Working Copy の画面を出す。
    pub working_copy: bool,
    /// 開いたら History でこのコミットを選ぶ（書き込む先の worktree を替えたタブ）。
    pub select: Option<String>,
    /// 開いたら、このファイルの File History を出す（File History のタブ）。
    pub file_history: Option<String>,
    /// マージエディタを別の窓に出した: このファイルを開き、`merge_text` があれば Result をその中身にする。
    pub merge: Option<String>,
    pub merge_text: Option<PathBuf>,
    /// 戻す窓のタブ（`--tabs`。`tabs::saved` の形）。
    pub tabs: Option<String>,
}

fn parse() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        repo: PathBuf::from("."),
        host: "app".into(),
        measure: None,
        measure_input: None,
        draw_on_input: false,
        raw_input: false,
        keep_presenting: false,
        child_hwnd: false,
        options: Options {
            waitable: true,
            sync: 1,
            tearing: false,
        },
        out: None,
        repositories: false,
        repo_given: false,
        place: None,
        working_copy: false,
        select: None,
        file_history: None,
        merge: None,
        merge_text: None,
        tabs: None,
    };
    let mut rows = measure::ROWS_PER_FRAME;
    let mut measuring = false;
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} に値がない"));
        match a.as_str() {
            "--host" => parsed.host = value()?,
            "--measure-scroll" => measuring = true,
            "--scroll-rows" => rows = value()?.parse().map_err(|e| format!("{e}"))?,
            "--measure-input" => {
                parsed.measure_input = Some(value()?.parse().map_err(|e| format!("{e}"))?)
            }
            "--draw-on-input" => parsed.draw_on_input = true,
            "--keep-presenting" => parsed.keep_presenting = true,
            "--child-hwnd" => {
                parsed.child_hwnd = true;
                parsed.raw_input = true;
                parsed.draw_on_input = true;
            }
            "--raw-input" => {
                parsed.raw_input = true;
                parsed.draw_on_input = true;
            }
            "--no-waitable" => parsed.options.waitable = false,
            "--sync" => parsed.options.sync = value()?.parse().map_err(|e| format!("{e}"))?,
            "--tearing" => parsed.options.tearing = true,
            "--out" => parsed.out = Some(value()?.into()),
            "--repositories" => parsed.repositories = true,
            "--working-copy" => parsed.working_copy = true,
            // 起動したときに戻す窓のタブ（`tabs::saved` の形。1 つ目の窓が残りの窓を開くときに渡す）。
            "--tabs" => parsed.tabs = Some(value()?),
            // マージエディタを別の窓に出す（そのファイルと、打ちかけの Result を書いたファイル）。
            "--merge" => parsed.merge = Some(value()?),
            "--merge-text" => parsed.merge_text = Some(value()?.into()),
            // 試し: Windows の設定を変えずに外観を決める。
            "--appearance" => match value()?.as_str() {
                "dark" => palette::force(true),
                "light" => palette::force(false),
                other => return Err(format!("--appearance は light か dark: {other}")),
            },
            "--place" => {
                let numbers: Vec<i32> = value()?
                    .split(',')
                    .map(|n| n.trim().parse().map_err(|e| format!("--place: {e}")))
                    .collect::<Result<_, _>>()?;
                parsed.place = Some(
                    numbers
                        .try_into()
                        .map_err(|_| "--place は x,y,幅,高さ".to_owned())?,
                );
            }
            // SAFETY: スレッドを作る前（引数を読んでいるところ）で、ほかに環境を読む者はいない。
            "--profile" => unsafe { std::env::set_var("AMONHEN_PROFILE", "1") },
            _ if a.starts_with("--") => return Err(format!("知らない引数: {a}")),
            _ => {
                parsed.repo = a.into();
                parsed.repo_given = true;
            }
        }
    }
    parsed.measure = measuring.then_some(rows);
    Ok(parsed)
}

fn main() {
    let started = Instant::now();
    // git・ssh が資格情報を尋ねてきた（`GIT_ASKPASS`・`SSH_ASKPASS` がこの実行ファイル）。
    if let Some(prompt) = askpass::prompt() {
        amonhen_model::i18n::set_default(language());
        askpass::run(prompt);
    }
    let mut raw = std::env::args().skip(1);
    if raw.next().as_deref() == Some("--listen") {
        if let Err(e) = listen::run(raw.next().unwrap_or_else(|| "listen.txt".into()).into()) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    amonhen_model::i18n::set_default(language());
    // git・ssh が尋ねる資格情報は、この実行ファイルの小さな窓で尋ねる（ほかのスレッドを動かす前に）。
    askpass::install();
    // status で git の fsmonitor を使うか（Settings > Performance。変えたら Settings が渡し直す）。
    amonhen_backend_cli::set_fsmonitor(prefs::bool(prefs::USE_FSMONITOR));
    // Settings > Git > Environment Variables で選んだ名前を、git と ssh に渡す（ほかのスレッドを動かす前に）。
    settingsview::apply_env_added();
    render::set_one_line(prefs::bool(prefs::COMPACT_HISTORY_ROWS));
    render::set_history_items(prefs::history_items());
    render::set_highlight_ancestors(prefs::bool(prefs::HIGHLIGHT_ANCESTORS));
    palette::refresh();
    let mut args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    // 引数なしで起動したら、終了したときに開いていた窓を開き直す（1 つ目を最初の窓に、残りも同じプロセスの窓に。なくなった
    // リポジトリは除く）。記録がなければ最後に閉じた窓のタブ、それもなければ最後のリポジトリ、それもなくなって
    // いれば今のフォルダ。
    let mut restored = (Vec::new(), 0);
    let mut more: Vec<(Vec<String>, usize)> = Vec::new();
    let exists = |l: &str| std::path::Path::new(l).join(".git").exists();
    if let Some(text) = args.tabs.take() {
        restored = tabs::parse_saved(&text, exists);
    } else if !args.repo_given && !args.repositories {
        let mut windows = session::take()
            .into_iter()
            .filter(|w| !tabs::parse_saved(w, exists).0.is_empty());
        if let Some(first) = windows.next() {
            restored = tabs::parse_saved(&first, exists);
            more = windows.map(|w| tabs::parse_saved(&w, exists)).collect();
        } else if let Some(text) = prefs::string(prefs::LAST_TABS) {
            restored = tabs::parse_saved(&text, exists);
        }
    }
    if !args.repo_given && !args.repositories {
        if let Some(first) = restored.0.get(restored.1) {
            args.repo = first.into();
        } else if let Some(last) = prefs::string(prefs::LAST_REPOSITORY)
            && std::path::Path::new(&last).join(".git").exists()
        {
            args.repo = last.into();
        }
    }
    let opened = std::fs::canonicalize(&args.repo).unwrap_or_else(|_| args.repo.clone());
    let opened = opened.display().to_string();
    let opened = opened.strip_prefix(r"\\?\").unwrap_or(&opened).to_owned();
    tabs::init(
        &restored.0,
        restored.1,
        &opened,
        args.repositories,
        args.working_copy,
    );
    tabs::set_merge(
        args.merge.clone(),
        args.merge_text.as_ref().map(|p| p.display().to_string()),
    );
    let more: Vec<u64> = more
        .iter()
        .map(|(locations, active)| tabs::new_window(locations, *active))
        .collect();
    let result = match args.host.as_str() {
        "app" => app::run(args, started, more),
        "win32" => win32::run(args, started),
        "winui" => winui::run(args, started),
        other => Err(format!("知らない --host: {other}")),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

enum Phase {
    /// History と最初の画面の件名を待っている。
    Waiting,
    /// 測る前に何フレームか描いて落ち着かせる。
    Settling(u32),
    Running,
    Done,
}

/// どのウィンドウにも共通の、描く中身と測り方。
pub struct Session {
    pub data: Data,
    pub renderer: Renderer,
    pub surface: Option<Surface>,
    pub scale: f32,
    scroll_y: f32,
    /// 選んだ行と、そのコミット（History が替わったら、同じコミットの行を選び直す）。
    pub selected: Option<u32>,
    selected_id: Option<amonhen_model::Oid>,
    /// 直前の `poll` で History が替わった。
    pub history_replaced: bool,
    dirty: bool,
    measure: Option<Measure>,
    input: Option<InputLatency>,
    phase: Phase,
    started: Instant,
    first_paint: bool,
    host: &'static str,
    out: Option<PathBuf>,
    keep_presenting: bool,
}

impl Session {
    pub fn new(
        args: &Args,
        host: &'static str,
        started: Instant,
        wake: data::Wake,
    ) -> Result<Self, String> {
        let renderer = Renderer::new().map_err(|e| format!("Direct2D を用意できない: {e}"))?;
        let session = Self {
            data: Data::open(args.repo.clone(), wake),
            renderer,
            surface: None,
            scale: 1.0,
            scroll_y: 0.0,
            selected: None,
            selected_id: None,
            history_replaced: false,
            dirty: true,
            measure: args.measure.map(Measure::new),
            input: args.measure_input.map(InputLatency::new),
            phase: Phase::Waiting,
            started,
            first_paint: false,
            keep_presenting: args.keep_presenting,
            host,
            out: args.out.clone(),
        };
        session.report(&format!(
            "host {host}, waitable {}, sync {}, tearing {} (supported {})",
            args.options.waitable,
            args.options.sync,
            args.options.tearing,
            session.renderer.tearing_supported()
        ));
        Ok(session)
    }

    fn report(&self, line: &str) {
        println!("{line}");
        if let Some(out) = &self.out {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(out)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn needs_frame(&self) -> bool {
        self.dirty
            || self.keep_presenting
            || matches!(self.phase, Phase::Settling(_) | Phase::Running)
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// History の窓の高さ（DIP。まだ描いていなければ 0）。
    pub fn height(&self) -> f32 {
        self.surface
            .as_ref()
            .map_or(0.0, |s| s.height as f32 / self.scale)
    }

    fn max_scroll(&self) -> f32 {
        (self.data.row_count() as f32 * row_height() - self.height()).max(0.0)
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, self.max_scroll());
        self.dirty = true;
    }

    /// History の行の見せ方（1 行か・出す項目）を変える。行の高さが変わることがあるので、上の端の行を保つ。
    pub fn set_display(&mut self, change: impl FnOnce()) {
        let row = self.scroll_y / row_height();
        change();
        self.scroll_to(row * row_height());
        self.invalidate();
    }

    pub fn scroll_to(&mut self, y: f32) {
        self.scroll_y = y.clamp(0.0, self.max_scroll());
        self.dirty = true;
    }

    /// 行の上の端の、窓の中の高さ（DIP）。
    pub fn row_top(&self, row: u32) -> f32 {
        row as f32 * row_height() - self.scroll_y
    }

    /// History の幅（DIP。まだ描いていなければ 0）。
    pub fn width(&self) -> f32 {
        self.surface
            .as_ref()
            .map_or(0.0, |s| s.width as f32 / self.scale.max(0.1))
    }

    /// `row` の行の右端の CI の印の上なら、そのコミットと状態（GTK4 版の `ci_mark`）。
    pub fn ci_mark_at(&self, row: u32, x: f32) -> Option<(amonhen_model::Oid, github::Ci)> {
        if render::history_items() & render::items::CI == 0 || !render::on_ci_mark(self.width(), x)
        {
            return None;
        }
        let id = *self.data.history.as_ref()?.graph.ids().get(row as usize)?;
        Some((id, *self.data.ci.get(&id)?))
    }

    /// 窓の中の高さ `y`（DIP）にある行。
    pub fn row_at(&self, y: f32) -> Option<u32> {
        let row = ((self.scroll_y + y) / row_height()).floor();
        (row >= 0.0 && (row as u32) < self.data.row_count()).then_some(row as u32)
    }

    /// 行を選び、見える所までスクロールする（複数選んでいれば、1 つに戻す）。
    /// History が届いたら、コミット `id` を選ぶ（まだ読んでいないとき。届いたら `poll` が選び直す）。
    pub fn select_when_read(&mut self, id: amonhen_model::Oid) {
        self.selected_id = Some(id);
        if let Some(row) = self.data.row_of(&id) {
            self.reveal(row);
        }
    }

    pub fn select(&mut self, row: u32) {
        self.data.multi.clear();
        let row = row.min(self.data.row_count().saturating_sub(1));
        self.selected = Some(row);
        self.selected_id = self
            .data
            .history
            .as_ref()
            .and_then(|h| h.graph.ids().get(row as usize).copied());
        let top = row as f32 * row_height();
        if top < self.scroll_y {
            self.scroll_to(top);
        } else if top + row_height() > self.scroll_y + self.height() {
            self.scroll_to(top + row_height() - self.height());
        }
        self.dirty = true;
    }

    /// 行を選び、見えていなければ真ん中に出す（サイドバーで ref を選んだとき）。
    pub fn reveal(&mut self, row: u32) {
        let top = row as f32 * row_height();
        if top < self.scroll_y || top + row_height() > self.scroll_y + self.height() {
            self.scroll_to(top - (self.height() - row_height()) / 2.0);
        }
        self.select(row);
    }

    /// Ctrl+クリック: `row` を選んだ中に足す・外す。1 つになったら、ふつうの選択に戻す。
    pub fn toggle_select(&mut self, row: u32) {
        let mut rows = std::mem::take(&mut self.data.multi);
        if rows.is_empty()
            && let Some(current) = self.selected
        {
            rows.push(current);
        }
        match rows.binary_search(&row) {
            Ok(i) => {
                rows.remove(i);
            }
            Err(i) => rows.insert(i, row),
        }
        match rows.as_slice() {
            [] => {}
            [only] => self.select(*only),
            _ => {
                self.select(row);
                self.data.multi = rows;
            }
        }
        self.dirty = true;
    }

    /// Shift+クリック: 選んでいる行から `row` までを選ぶ。
    pub fn select_range(&mut self, row: u32) {
        let Some(anchor) = self.selected else {
            return self.select(row);
        };
        let rows: Vec<u32> = (anchor.min(row)..=anchor.max(row)).collect();
        self.select(row);
        if rows.len() > 1 {
            self.data.multi = rows;
        }
        self.dirty = true;
    }

    /// 選んだ行を `delta` 行動かす（何も選んでいなければ先頭）。
    pub fn move_selection(&mut self, delta: i64) {
        let count = self.data.row_count();
        if count == 0 {
            return;
        }
        let row = match self.selected {
            Some(row) => (row as i64 + delta).clamp(0, count as i64 - 1) as u32,
            None => 0,
        };
        self.select(row);
    }

    /// 1 画面分の行の数。
    pub fn page_rows(&self) -> i64 {
        ((self.height() / row_height()) as i64 - 1).max(1)
    }

    pub fn page(&self) -> f32 {
        self.height() * 0.9
    }

    fn visible(&self) -> std::ops::Range<u32> {
        let count = self.data.row_count();
        let first = ((self.scroll_y / row_height()) as u32).min(count);
        let last = (((self.scroll_y + self.height()) / row_height()).ceil() as u32).min(count);
        first..last
    }

    /// クリックで 1 画面分進める（`--measure-input` の入力）。`at` は受け取った時刻（QPC）。
    pub fn clicked(&mut self, at: i64) {
        if let Some(input) = &mut self.input {
            input.received(at);
        }
        let page = self.page();
        if self.scroll_y + page > self.max_scroll() {
            self.scroll_to(0.0);
        } else {
            self.scroll_by(page);
        }
    }

    /// 画面に出るのを待っている入力があるか（あれば `poll_input` を呼び続ける）。
    pub fn awaiting_display(&self) -> bool {
        self.input.as_ref().is_some_and(|i| i.waiting())
    }

    /// 画面に出た入力を書き出す。決めた回数を測り終えたら `true`。
    pub fn poll_input(&mut self) -> bool {
        let (Some(input), Some(surface)) = (&mut self.input, &self.surface) else {
            return false;
        };
        let found = input.poll(&surface.chain);
        let done = input.finished();
        let summary = done.then(|| input.summary());
        for (received, displayed) in found {
            self.report(&format!("input received={received} displayed={displayed}"));
        }
        if let Some(summary) = summary {
            self.report(&format!("[{}] {summary}", self.host));
        }
        done
    }

    /// 裏のスレッドから届いたものを取り込む。
    pub fn poll(&mut self) -> Result<(), String> {
        if self.data.poll()? {
            self.dirty = true;
        }
        self.history_replaced = std::mem::take(&mut self.data.history_changed);
        if self.history_replaced {
            // 同じコミットを選び直す（なくなっていれば選ばない）。スクロールの位置は行の番号のまま。
            self.selected = self.selected_id.and_then(|id| self.data.row_of(&id));
            if self.selected.is_none() {
                self.selected_id = None;
            }
            self.scroll_by(0.0);
            // 範囲を変えたら、選んだコミットを見せる（なければ先頭）。
            if std::mem::take(&mut self.data.scope_changed) {
                match self.selected {
                    Some(row) => self.reveal(row),
                    None => self.scroll_to(0.0),
                }
            }
        }
        Ok(())
    }

    /// 1 フレーム描いて出す。測り終えたら `true`。
    pub fn frame(&mut self) -> Result<bool, String> {
        if self.surface.is_none() {
            return Ok(false);
        }
        let start = match (&self.phase, &mut self.measure) {
            (Phase::Running, Some(m)) => {
                let (t, dy) = (m.frame_started(), m.rows_per_frame * row_height());
                self.scroll_by(dy);
                t
            }
            _ => measure::now(),
        };
        let surface = self.surface.as_mut().expect("surface");
        self.renderer
            .draw(
                surface,
                &mut self.data,
                self.scroll_y,
                self.scale,
                self.selected,
            )
            .map_err(|e| format!("描けない: {e}"))?;
        surface.present().map_err(|e| format!("出せない: {e}"))?;
        if let Some(input) = &mut self.input {
            input.presented(&surface.chain);
        }
        // 祖先が描く行まで届いていなければ、次のフレームで続ける。
        self.dirty = self.renderer.lineage_pending;

        let ready = self.data.has_rows(self.visible());
        if ready && !self.first_paint {
            self.first_paint = true;
            self.report(&format!(
                "first History paint: {:.0} ms after start ({} rows)",
                self.started.elapsed().as_secs_f64() * 1000.0,
                self.data.row_count()
            ));
        }
        match self.phase {
            Phase::Waiting if ready && self.measure.is_some() => self.phase = Phase::Settling(30),
            Phase::Settling(0) => self.phase = Phase::Running,
            Phase::Settling(n) => self.phase = Phase::Settling(n - 1),
            Phase::Running => {
                let at_end = self.scroll_y >= self.max_scroll();
                let m = self.measure.as_mut().expect("measure");
                m.frame_presented(start, &self.surface.as_ref().expect("surface").chain);
                if m.finished() || at_end {
                    self.phase = Phase::Done;
                    let summary = m.summary();
                    self.report(&format!("[{}] {summary}", self.host));
                    if let Some(p) = &self.renderer.profile {
                        self.report(&render::profile_summary(p));
                    }
                    return Ok(true);
                }
            }
            _ => {}
        }
        Ok(false)
    }
}

/// アプリの言語: `AMONHEN_LANG` があればそれ、なければ Windows の表示の言語（`GetUserDefaultUILanguage` の言語の名前）。
fn language() -> amonhen_model::i18n::Lang {
    use amonhen_model::i18n::Lang;
    use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
    if let Some(lang) = std::env::var("AMONHEN_LANG").ok().filter(|v| !v.is_empty()) {
        return Lang::parse(&lang);
    }
    let mut name = [0u16; 85];
    // SAFETY: 名前を書く先の大きさを渡すだけ。
    let len = unsafe { LCIDToLocaleName(GetUserDefaultUILanguage() as u32, Some(&mut name), 0) };
    if len <= 1 {
        return Lang::En;
    }
    Lang::parse(&String::from_utf16_lossy(&name[..len as usize - 1]))
}
