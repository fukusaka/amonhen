//! SSH 先のフォルダを列で選ぶ画面（GTK4 版の `filepicker.rs`、macOS 版の `FilePicker.swift` のうち、SSH 先の
//! フォルダを選ぶ分）。上にパスの欄（打ち込める。`~` はその機械のホーム）、下に根（`/`・`C:\`）から選んだところまで
//! の列。中身は SSH 先でリポジトリなしに動かしたエンジンの `listDirectory` が読む。リポジトリは緑で出す。

use std::sync::Arc;

use amonhen_core::Repository;
use amonhen_model::tr;
use serde_json::{Value, json};
use windows_reactor::*;

/// 一度に出す列の数。
const VISIBLE: usize = 3;

/// 選んだものを何に使うか。
#[derive(Clone)]
pub enum Purpose {
    /// SSH 接続を追加するダイアログのパス（決めたら、そのダイアログに戻す）。
    AddSsh,
    /// その SSH 先でスキャンするフォルダ（Settings > SSH Hosts・Places）。
    ScanRoot,
}

/// 列の 1 行（フォルダだけ）。
#[derive(Clone)]
pub struct Item {
    pub name: String,
    pub repository: bool,
}

/// 列 1 つ: そのフォルダと中身（読むまでは None）と、選んでいる行の名前。
#[derive(Clone)]
pub struct Column {
    pub path: String,
    pub items: Option<Result<Vec<Item>, String>>,
    pub selected: Option<String>,
}

#[derive(Clone)]
pub struct Picker {
    pub purpose: Purpose,
    pub host: String,
    /// 開いたときのパス（取りやめたら戻す）。
    pub start: String,
    pub repo: Option<Arc<Repository>>,
    /// つながらなかった理由。
    pub error: Option<String>,
    pub home: Option<String>,
    pub columns: Vec<Column>,
    /// パスの欄（選んだら、そのフォルダにする）と、欄を作り直す数。
    pub typed: String,
    pub epoch: u32,
}

pub enum Event {
    Connected(Result<Arc<Repository>, String>),
    /// 読んだフォルダ（頼んだパスと答え）。
    Listed(String, Result<Value, String>),
    /// 列の行を選んだ（列と、その列の行の番号）。
    Pick(usize, Option<usize>),
    Typed(String),
    /// パスの欄のフォルダへ。
    Go,
}

/// 裏でする仕事。
pub enum Job {
    Connect {
        location: String,
        options: amonhen_remote::ssh::HostOptions,
        local: bool,
    },
    List(Arc<Repository>, String),
}

impl Job {
    pub fn run(self) -> Event {
        match self {
            Job::Connect {
                location,
                options,
                local,
            } => Event::Connected(
                crate::remote::open(&location, options, local, &|| {}).map(Arc::new),
            ),
            Job::List(repo, path) => {
                let command = json!({ "op": "listDirectory", "path": &path });
                let reply: Value =
                    serde_json::from_str(&repo.run_json(&command.to_string())).unwrap_or_default();
                let result = match reply["error"].as_str() {
                    Some(e) => Err(e.to_owned()),
                    None => Ok(reply["info"].clone()),
                };
                Event::Listed(path, result)
            }
        }
    }
}

/// Windows のパスか（`C:\…`・`C:/…`・`\\server\…`）。
fn windows(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with("\\\\") || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// 根から `path` までのフォルダ（根を含む。`/a/b` なら `/`・`/a`・`/a/b`、`C:\a` なら `C:\`・`C:\a`）。
pub fn ancestors(path: &str) -> Vec<String> {
    let (root, rest, sep) = if windows(path) {
        let p = path.replace('/', "\\");
        let root = p.get(..3).filter(|r| r.ends_with('\\')).map_or_else(
            || format!("{}\\", p.get(..2).unwrap_or_default()),
            str::to_owned,
        );
        let rest = p
            .get(root.len().min(p.len())..)
            .unwrap_or_default()
            .to_owned();
        (root, rest, '\\')
    } else {
        ("/".to_owned(), path.trim_start_matches('/').to_owned(), '/')
    };
    let mut out = vec![root.clone()];
    let mut at = root;
    for part in rest.split(['/', '\\']).filter(|p| !p.is_empty()) {
        if !at.ends_with(sep) {
            at.push(sep);
        }
        at.push_str(part);
        out.push(at.clone());
    }
    out
}

/// `dir` の下の `name`。
fn join(dir: &str, name: &str) -> String {
    let sep = if windows(dir) { '\\' } else { '/' };
    if dir.ends_with(['/', '\\']) {
        format!("{dir}{name}")
    } else {
        format!("{dir}{sep}{name}")
    }
}

impl Picker {
    /// `host` の `start`（空ならホーム）から選ぶ。`repo` がその SSH 先にもうつないであるエンジンなら使い、なければ
    /// つなぎに行く。最初の仕事を返す。
    pub fn new(
        purpose: Purpose,
        host: &str,
        start: &str,
        repo: Option<Arc<Repository>>,
    ) -> (Self, Job) {
        let location = format!("ssh://{host}");
        let job = Job::Connect {
            options: crate::remote::options_for(&location),
            local: crate::remote::checks_signatures_locally(&location),
            location,
        };
        let start = start.trim().to_owned();
        let typed = if start.is_empty() {
            "~".to_owned()
        } else {
            start.clone()
        };
        let job = match &repo {
            Some(repo) => Job::List(repo.clone(), typed.clone()),
            None => job,
        };
        (
            Self {
                purpose,
                host: host.to_owned(),
                typed,
                start,
                repo,
                error: None,
                home: None,
                columns: Vec::new(),
                epoch: 0,
            },
            job,
        )
    }

    /// 選んだフォルダ（最後の列。まだなければ None）。
    pub fn chosen(&self) -> Option<&str> {
        self.columns.last().map(|c| c.path.as_str())
    }

    /// 選んだフォルダを、ホームの下なら `~/…` で（ssh の場所に書く形）。
    pub fn chosen_shown(&self) -> Option<String> {
        let path = self.chosen()?;
        Some(match &self.home {
            Some(home) if path == home => "~".to_owned(),
            Some(home)
                if path.len() > home.len()
                    && path.starts_with(home.as_str())
                    && path[home.len()..].starts_with(['/', '\\']) =>
            {
                format!("~/{}", path[home.len() + 1..].replace('\\', "/"))
            }
            _ => path.to_owned(),
        })
    }

    fn list(&self, path: String) -> Option<Job> {
        Some(Job::List(self.repo.clone()?, path))
    }

    pub fn handle(&mut self, event: Event) -> Vec<Job> {
        let mut jobs = Vec::new();
        match event {
            Event::Connected(Ok(repo)) => {
                self.repo = Some(repo);
                self.error = None;
                jobs.extend(self.list(self.typed.clone()));
            }
            Event::Connected(Err(e)) => self.error = Some(e),
            Event::Listed(path, result) => {
                // 初めて読んだ（打ったパス）なら、根からそのフォルダまでの列を作る。
                if self.columns.is_empty() {
                    match result {
                        Ok(info) => {
                            let at = info["path"].as_str().unwrap_or(&path).to_owned();
                            self.home = info["home"].as_str().map(str::to_owned);
                            let chain = ancestors(&at);
                            for (i, dir) in chain.iter().enumerate() {
                                let selected = chain.get(i + 1).and_then(|next| {
                                    next[dir.len()..]
                                        .trim_start_matches(['/', '\\'])
                                        .split(['/', '\\'])
                                        .next()
                                        .map(str::to_owned)
                                });
                                self.columns.push(Column {
                                    path: dir.clone(),
                                    items: (i + 1 == chain.len()).then(|| Ok(items_of(&info))),
                                    selected,
                                });
                            }
                            for dir in &chain[..chain.len() - 1] {
                                jobs.extend(self.list(dir.clone()));
                            }
                            self.set_typed(self.chosen_shown().unwrap_or_default());
                        }
                        Err(e) => self.error = Some(e),
                    }
                } else if let Some(column) = self.columns.iter_mut().find(|c| c.path == path) {
                    column.items = Some(result.map(|info| items_of(&info)));
                }
            }
            Event::Pick(at, Some(i)) => {
                let Some(name) = self
                    .columns
                    .get(at)
                    .and_then(|c| c.items.as_ref()?.as_ref().ok()?.get(i))
                    .map(|item| item.name.clone())
                else {
                    return jobs;
                };
                if self.columns[at].selected.as_ref() == Some(&name) && self.columns.len() > at + 1
                {
                    return jobs;
                }
                self.columns.truncate(at + 1);
                let child = join(&self.columns[at].path, &name);
                self.columns[at].selected = Some(name);
                self.columns.push(Column {
                    path: child.clone(),
                    items: None,
                    selected: None,
                });
                jobs.extend(self.list(child));
                self.set_typed(self.chosen_shown().unwrap_or_default());
            }
            Event::Pick(_, None) => {}
            Event::Typed(text) => self.typed = text,
            Event::Go => {
                self.columns.clear();
                jobs.extend(self.list(self.typed.trim().to_owned()));
            }
        }
        jobs
    }

    fn set_typed(&mut self, text: String) {
        self.typed = text;
        self.epoch += 1;
    }
}

/// `listDirectory` の答えのうち、フォルダ（`.` で始まるものは除く。リポジトリの印つき）。
fn items_of(info: &Value) -> Vec<Item> {
    info["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["kind"] == "folder")
        .filter_map(|e| {
            let name = e["name"].as_str()?;
            (!name.starts_with('.')).then(|| Item {
                name: name.to_owned(),
                repository: e["repository"].as_bool() == Some(true),
            })
        })
        .collect()
}

pub struct Callbacks<'a> {
    pub pick: Box<dyn Fn(usize) -> Callback<Option<usize>> + 'a>,
    pub typed: Callback<String>,
    pub go: Callback<()>,
}

pub fn view(picker: &Picker, callbacks: &Callbacks<'_>) -> View {
    let head = Grid::new()
        .columns([GridLength::Star(1.0), GridLength::Auto])
        .column_spacing(6.0)
        .children((
            View::keyed_fragment([(
                format!("typed-{}", picker.epoch),
                TextBox::new()
                    .text(picker.typed.clone())
                    .placeholder_text(tr!("Go to a folder"))
                    .on_text_changed(callbacks.typed.clone())
                    .grid_column(0),
            )]),
            Button::new()
                .grid_column(1)
                .is_enabled(picker.repo.is_some())
                .on_click(callbacks.go.clone())
                .content(TextBlock::new().text("→")),
        ));
    let body: View = if let Some(error) = &picker.error {
        TextBlock::new()
            .text(error.clone())
            .text_wrapping(TextWrapping::Wrap)
            .foreground(Brush::Solid(Color::rgb(0xd0, 0x3a, 0x3a)))
            .into()
    } else if picker.columns.is_empty() {
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .children((
                ProgressRing::new().is_active(true).width(16.0).height(16.0),
                TextBlock::new().text(tr!("Connecting to {host}…", host = &picker.host)),
            ))
    } else {
        let green = Color::rgb(0x2e, 0x9e, 0x4f);
        // ダイアログの幅に収まる、終わりの 3 列だけ（選んだ所が見えるように。前の列はパスの欄に出ている）。
        let first = picker.columns.len().saturating_sub(VISIBLE);
        let columns = picker
            .columns
            .iter()
            .enumerate()
            .skip(first)
            .map(|(at, column)| {
                let content: View = match &column.items {
                    None => ProgressRing::new()
                        .is_active(true)
                        .width(16.0)
                        .height(16.0)
                        .into(),
                    Some(Err(e)) => TextBlock::new()
                        .text(e.clone())
                        .text_wrapping(TextWrapping::Wrap)
                        .opacity(0.7)
                        .font_size(12.0)
                        .into(),
                    Some(Ok(items)) => {
                        let selected = column
                            .selected
                            .as_ref()
                            .and_then(|s| items.iter().position(|i| &i.name == s));
                        ListView::new()
                            .selected_index(selected)
                            .on_selection_changed((callbacks.pick)(at))
                            .collection_slot(
                                ListViewSlot::Items,
                                items.iter().map(|item| {
                                    let text = TextBlock::new()
                                        .text(format!("{} ›", item.name))
                                        .font_size(13.0)
                                        .text_trimming(TextTrimming::CharacterEllipsis);
                                    let text = if item.repository {
                                        text.foreground(Brush::Solid(green))
                                    } else {
                                        text
                                    };
                                    (item.name.clone(), ListViewItem::new().content(text))
                                }),
                            )
                    }
                };
                (
                    column.path.clone(),
                    Border::new()
                        .width(150.0)
                        .height(300.0)
                        .border_thickness(Thickness::new(0.0, 0.0, 1.0, 0.0))
                        .border_brush(Brush::Solid(crate::palette::border()))
                        .content(content),
                )
            });
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .children([View::keyed_fragment(columns)])
    };
    StackPanel::new()
        .spacing(8.0)
        .width(460.0)
        .children((head, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_from_the_root() {
        assert_eq!(
            ancestors("/home/me/src"),
            ["/", "/home", "/home/me", "/home/me/src"]
        );
        assert_eq!(ancestors("/"), ["/"]);
        assert_eq!(
            ancestors(r"C:\Users\me"),
            [r"C:\", r"C:\Users", r"C:\Users\me"]
        );
        assert_eq!(ancestors("C:/src"), [r"C:\", r"C:\src"]);
        assert_eq!(join("/", "home"), "/home");
        assert_eq!(join(r"C:\", "src"), r"C:\src");
        assert_eq!(join(r"C:\src", "a"), r"C:\src\a");
    }

    #[test]
    fn chosen_folders_under_home_use_a_tilde() {
        let (mut picker, _) = Picker::new(Purpose::ScanRoot, "box", "", None);
        picker.home = Some("/home/me".into());
        picker.columns.push(Column {
            path: "/home/me/src".into(),
            items: None,
            selected: None,
        });
        assert_eq!(picker.chosen_shown().as_deref(), Some("~/src"));
        picker.home = Some(r"C:\Users\me".into());
        picker.columns[0].path = r"C:\Users\me\prj\a".into();
        assert_eq!(picker.chosen_shown().as_deref(), Some("~/prj/a"));
        picker.columns[0].path = "/srv".into();
        assert_eq!(picker.chosen_shown().as_deref(), Some("/srv"));
    }
}
