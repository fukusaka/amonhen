//! Settings > Git > Programs（macOS 版の `MachineSettings.swift` の `ProgramsPage`）: git・gpg・ssh-keygen・gpgsm を
//! 同じ形で選ぶ（自動＝PATH の先頭／見つかったもの／その他…）。git はその機械（この機械か SSH 先）の Amon Hen の設定
//! （`settings.json`。そこの CLI・daemon・VSCode 拡張も従う。コアの `machineGit`・`setMachineGit`）、ほかは git の設定
//! （`gpg.program` など）。候補はコアの `programs` で、git を動かす機械（SSH 先ならそこ）で探す。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;
use serde_json::{Value, json};

use crate::reposettings::{Keys, Picker, RepoSettings, label, run_one, section};

/// 「その他…」の項目の値（値と混ざらないもの）。
const OTHER: &str = "\u{1}other";

/// 探すプログラムと、それを決める git の設定のキー（先に決まっているものを使い、どれもなければ最後のキーに書く）。
const CONFIG_PROGRAMS: [(&str, &[&str]); 3] = [
    ("gpg", &["gpg.openpgp.program", "gpg.program"]),
    ("ssh-keygen", &["gpg.ssh.program"]),
    ("gpgsm", &["gpg.x509.program"]),
];

/// プログラムの候補（コアの `programs`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub path: String,
    pub version: Option<String>,
}

impl Candidate {
    /// 出どころと版（Homebrew 2.56.0、/usr/bin 2.43.0）。SSH 先ではフォルダ。
    fn label(&self, on_host: bool) -> String {
        let source = if on_host {
            parent(&self.path)
        } else {
            // 出どころはリンクの先で見る（/usr/local/bin/gpg が MacGPG2 のものなど）。
            let real = std::fs::canonicalize(&self.path)
                .map_or_else(|_| self.path.clone(), |p| p.to_string_lossy().into_owned());
            source_of(&real)
        };
        // Apple の git は「2.54.0 (Apple Git-157)」と出すので、版は最初の語だけ。
        match self
            .version
            .as_deref()
            .and_then(|v| v.split_whitespace().next())
        {
            Some(v) => format!("{source} {v}"),
            None => source,
        }
    }
}

/// パスの親のフォルダ（`/` も `\` も区切りとみる。Windows の SSH 先）。
fn parent(path: &str) -> String {
    match path.rfind(['/', '\\']) {
        Some(0) => path[..1].to_owned(),
        Some(i) => path[..i].to_owned(),
        None => path.to_owned(),
    }
}

/// この機械のプログラムの出どころ（Homebrew・MacPorts など。わからなければフォルダ）。
fn source_of(path: &str) -> String {
    if path.starts_with("/opt/homebrew/")
        || path.starts_with("/usr/local/Cellar/")
        || path.starts_with("/home/linuxbrew/")
    {
        return "Homebrew".into();
    }
    if path.starts_with("/usr/local/MacGPG2/") {
        return "MacGPG2".into();
    }
    if path.starts_with("/opt/local/") {
        return "MacPorts".into();
    }
    if cfg!(target_os = "macos")
        && (path.starts_with("/usr/bin/")
            || path.starts_with("/Applications/Xcode")
            || path.starts_with("/Library/Developer/"))
    {
        return "Apple".into();
    }
    parent(path)
}

/// コアの `programs` の答えから。
pub(crate) fn candidates(info: &Value) -> Vec<Candidate> {
    info["programs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(Candidate {
                        path: p["path"].as_str()?.to_owned(),
                        version: p["version"].as_str().map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 使う git と、それをどこで決めたか（コアの `machineGit`）。
#[derive(Clone, Debug, Default)]
pub(crate) struct MachineGit {
    pub chosen: Option<String>,
    pub file: Option<String>,
    pub program: String,
    pub source: String,
    pub problem: Option<String>,
}

impl MachineGit {
    pub(crate) fn from_info(info: &Value) -> Self {
        let s = |k: &str| info[k].as_str().map(str::to_owned);
        Self {
            chosen: s("chosen"),
            file: s("file"),
            program: s("program").unwrap_or_default(),
            source: s("source").unwrap_or_default(),
            problem: s("problem"),
        }
    }

    /// 設定元の欄: 決めたところ。
    fn source_label(&self) -> &'static str {
        match self.source.as_str() {
            "settings" => "Amon Hen",
            "environment" => "AMONHEN_GIT",
            "fallback" => "AMONHEN_FALLBACK_GIT",
            _ => "PATH",
        }
    }
}

/// 選ぶ欄の項目: 自動（PATH の先頭）、見つかったもの、今の値（候補になければ）。「その他…」は別に。
fn options(found: &[Candidate], current: &str, on_host: bool) -> crate::reposettings::Options {
    let automatic = match found.first() {
        Some(c) => tr!("Automatic ({label})", label = c.label(on_host)),
        None => tr!("Automatic"),
    };
    let mut items: Vec<(String, String)> = found
        .iter()
        .map(|c| (c.path.clone(), c.label(on_host)))
        .collect();
    if !current.is_empty() && !found.iter().any(|c| c.path == current) {
        items.push((current.to_owned(), current.to_owned()));
    }
    vec![(None, vec![(String::new(), automatic)]), (None, items)]
}

/// 「その他…」: その機械の実行ファイルを選ぶ（SSH 先ならパスを入れてもらう）。最初は今の値か、PATH の先頭。
fn choose_other(
    s: &Rc<RepoSettings>,
    found: &[Candidate],
    current: &str,
    done: impl FnOnce(String) + 'static,
) {
    let initial = if current.is_empty() {
        found.first().map(|c| c.path.clone()).unwrap_or_else(|| {
            if s.host.is_some() {
                "/usr/bin".into()
            } else {
                "/usr/local/bin".into()
            }
        })
    } else {
        current.to_owned()
    };
    let (title, message) = match &s.host {
        Some(host) => (
            tr!("Program on {host}", host = host),
            tr!("Choose the program on the host."),
        ),
        None => (tr!("Program"), tr!("Choose the program on this computer.")),
    };
    s.choose_path(
        crate::filepicker::Kind::Executable,
        &title,
        &message,
        initial,
        done,
    );
}

/// Programs のページ。
pub fn page(s: &Rc<RepoSettings>) -> gtk::ScrolledWindow {
    let found: Rc<RefCell<HashMap<&'static str, Vec<Candidate>>>> = Rc::default();
    let mut rows: Vec<gtk::Widget> = vec![git_row(s, found.clone())];
    for (name, keys) in CONFIG_PROGRAMS {
        rows.push(config_row(s, name, keys, found.clone()));
    }
    let widget = crate::reposettings::page(&[section(None, &rows)]);
    // 候補は裏で探す（版を読むのにプログラムを動かすので）。探し終えたら欄を出し直す。
    let (repo, w) = (s.repo.clone(), s.weak());
    glib::spawn_future_local(async move {
        let names = ["git", "gpg", "ssh-keygen", "gpgsm"];
        let result = gtk::gio::spawn_blocking(move || {
            names
                .iter()
                .map(|&name| {
                    let info =
                        crate::reposettings::call(&repo, &json!({"op": "programs", "name": name}))
                            .unwrap_or_default();
                    (name, candidates(&info))
                })
                .collect::<HashMap<_, _>>()
        })
        .await
        .unwrap_or_default();
        found.replace(result);
        if let Some(s) = w.upgrade() {
            s.refresh();
        }
    });
    widget
}

/// git の設定に書くプログラムの行（設定元・↩︎・ⓘ はリポジトリの設定の行と同じ）。
fn config_row(
    s: &Rc<RepoSettings>,
    name: &'static str,
    keys: &'static [&'static str],
    found: Rc<RefCell<HashMap<&'static str, Vec<Candidate>>>>,
) -> gtk::Widget {
    // 決めてあるキーを先に（設定元はその値のもの）。どれもなければ最後のキーに書く。
    let key_of = move |s: &RepoSettings| -> String {
        keys.iter()
            .find(|k| s.effective(k).is_some())
            .unwrap_or(&keys[keys.len() - 1])
            .to_string()
    };
    let k: Keys = Rc::new(move |s| vec![key_of(s)]);
    let row = s.row(name, "", k, Rc::new(|_, v| v.to_owned()), None);
    let (w, f) = (s.weak(), found.clone());
    let picker = Picker::new(move |value| {
        let Some(s) = w.upgrade() else { return };
        if s.quiet() {
            return;
        }
        let key = key_of(&s);
        let current = s.local(&key).unwrap_or_default();
        if value == OTHER {
            let list = f.borrow().get(name).cloned().unwrap_or_default();
            let (w, start) = (s.weak(), current.clone());
            choose_other(&s, &list, &start, move |path| {
                if let Some(s) = w.upgrade()
                    && path != current
                {
                    s.write(vec![(key, Some(path))]);
                }
            });
        } else if value != current {
            s.write(vec![(key, (!value.is_empty()).then_some(value))]);
        }
    });
    picker.button.set_size_request(300, -1);
    row.control.append(&picker.button);
    s.on_refresh(move |s| {
        let current = s.local(&key_of(s)).unwrap_or_default();
        let list = found.borrow().get(name).cloned().unwrap_or_default();
        picker.set(
            &options(&list, &current, s.host.is_some()),
            &[(OTHER.to_owned(), tr!("Other…"))],
            &current,
        );
        picker.button.set_tooltip_text(Some(if current.is_empty() {
            list.first().map_or("", |c| c.path.as_str())
        } else {
            &current
        }));
    });
    row.widget.upcast()
}

/// git の行: 値はその機械の Amon Hen の設定（git の設定ではない）。行の形はほかと同じ。選んだ git がなければ、
/// その下に理由（git を動かす操作はエラーになる）。
fn git_row(
    s: &Rc<RepoSettings>,
    found: Rc<RefCell<HashMap<&'static str, Vec<Candidate>>>>,
) -> gtk::Widget {
    let git: Rc<RefCell<MachineGit>> = Rc::default();
    let widget = gtk::Box::new(gtk::Orientation::Vertical, 4);
    widget.set_margin_top(4);
    widget.set_margin_bottom(4);
    widget.set_margin_start(4);
    widget.set_margin_end(4);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let q = label("git", &[]);
    q.set_hexpand(true);
    line.append(&q);
    let warning = label("", &["settings-warning"]);
    warning.set_wrap(true);
    warning.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    warning.set_margin_start(8);
    warning.set_selectable(true);
    warning.set_visible(false);

    let source = label("", &["dim-label"]);
    let popover = gtk::Popover::new();
    // 読み直す（書いたあと・開いたとき）。
    let reload: Rc<dyn Fn()> = {
        let (repo, g, src, warn, pop, w) = (
            s.repo.clone(),
            git.clone(),
            source.clone(),
            warning.clone(),
            popover.clone(),
            s.weak(),
        );
        Rc::new(move || {
            let (repo, g, src, warn, pop, w) = (
                repo.clone(),
                g.clone(),
                src.clone(),
                warn.clone(),
                pop.clone(),
                w.clone(),
            );
            glib::spawn_future_local(async move {
                let info = run_one(&repo, json!({"op": "machineGit"}))
                    .await
                    .unwrap_or_default();
                let read = MachineGit::from_info(&info);
                let Some(s) = w.upgrade() else { return };
                src.set_text(read.source_label());
                warn.set_text(
                    &read
                        .problem
                        .as_deref()
                        .map(|p| format!("⚠ {p}"))
                        .unwrap_or_default(),
                );
                warn.set_visible(read.problem.is_some());
                pop.set_child(Some(&git_details(&s, &read)));
                g.replace(read);
                s.refresh();
            });
        })
    };

    let (w, g, f) = (s.weak(), git.clone(), found.clone());
    // 使う git を変える。設定の窓の書き換えとして Undo に前の値を積む（Ctrl+Shift+Z でやり直せる。macOS 版と
    // 同じ）。書いたら読み直す（下の `on_refresh`）。
    let set: Rc<dyn Fn(String)> = Rc::new(move |value: String| {
        let Some(s) = w.upgrade() else { return };
        let old = g.borrow().chosen.clone().unwrap_or_default();
        if value == old {
            return;
        }
        s.apply_changes(
            vec![crate::reposettings::Change {
                key: crate::reposettings::MACHINE_GIT.to_owned(),
                old: Some(old),
                new: Some(value),
            }],
            crate::reposettings::Record::New,
        );
    });
    let (w, g, st) = (s.weak(), git.clone(), set.clone());
    let picker = Picker::new(move |value| {
        let Some(s) = w.upgrade() else { return };
        if s.quiet() {
            return;
        }
        let current = g.borrow().chosen.clone().unwrap_or_default();
        if value == OTHER {
            let list = f.borrow().get("git").cloned().unwrap_or_default();
            let st = st.clone();
            choose_other(&s, &list, &current, move |path| st(path));
        } else {
            st(value);
        }
    });
    picker.button.set_size_request(300, -1);
    let control = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    control.set_valign(gtk::Align::Center);
    control.append(&picker.button);
    line.append(&control);

    // ↩︎（選んでいる間だけ）。
    let revert = gtk::Button::new();
    let icon = crate::icon::Icon::default();
    icon.set(crate::icon::Kind::Undo, false);
    revert.set_child(Some(&icon));
    revert.add_css_class("flat");
    revert.set_valign(gtk::Align::Center);
    revert.set_tooltip_text(Some(&tr!(
        "Stop overriding here. The value above takes over."
    )));
    let st = set.clone();
    revert.connect_clicked(move |_| st(String::new()));
    line.append(&revert);
    s.register_control("git", &control, &revert);
    source.set_xalign(1.0);
    source.set_size_request(150, -1);
    source.set_ellipsize(gtk::pango::EllipsizeMode::End);
    line.append(&source);
    let info = gtk::MenuButton::new();
    info.set_child(Some(&gtk::Label::new(Some("ⓘ"))));
    info.set_has_frame(false);
    info.set_valign(gtk::Align::Center);
    info.set_popover(Some(&popover));
    line.append(&info);
    widget.append(&line);
    widget.append(&warning);

    // 書いたあと（Undo・やり直しも）は、使う git を読み直す。
    let seen_writes = std::cell::Cell::new(s.writes());
    let r = reload.clone();
    s.on_refresh(move |s| {
        if s.writes() != seen_writes.get() {
            seen_writes.set(s.writes());
            r();
        }
        let current = git.borrow().chosen.clone().unwrap_or_default();
        let list = found.borrow().get("git").cloned().unwrap_or_default();
        picker.set(
            &options(&list, &current, s.host.is_some()),
            &[(OTHER.to_owned(), tr!("Other…"))],
            &current,
        );
        // 使う git のパス（Automatic なら最初の候補。macOS 版と同じ）。
        picker.button.set_tooltip_text(Some(if current.is_empty() {
            list.first().map_or("", |c| c.path.as_str())
        } else {
            &current
        }));
        let chosen = !current.is_empty();
        revert.set_opacity(if chosen { 1.0 } else { 0.0 });
        revert.set_can_target(chosen);
        revert.set_can_focus(chosen);
    });
    reload();
    widget.upcast()
}

/// ⓘ: 置き場所と値、今使う git。環境変数で上書きしていれば、そのこと。
fn git_details(s: &RepoSettings, git: &MachineGit) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.set_margin_top(6);
    b.set_margin_bottom(6);
    b.set_margin_start(6);
    b.set_margin_end(6);
    let add = |text: &str, css: &[&str]| {
        let l = label(text, css);
        l.set_selectable(true);
        l.set_wrap(true);
        l.set_max_width_chars(56);
        b.append(&l);
    };
    if let Some(file) = &git.file {
        let file = if s.host.is_none() {
            crate::reposettings::tilde(file)
        } else {
            file.clone()
        };
        add(&file, &["monospace"]);
    }
    add(
        &match &git.chosen {
            Some(c) => format!("\"git\": \"{c}\""),
            None => tr!("No git chosen: the first git on the PATH"),
        },
        &["monospace"],
    );
    add(
        &match &s.host {
            Some(host) => tr!(
                "The engine, the CLI and the VSCode extension on {host} use it too.",
                host = host
            ),
            None => tr!(
                "The CLI, the shared daemon and the VSCode extension on this machine use it too."
            ),
        },
        &["dim-label", "caption"],
    );
    if git.source == "environment" {
        add(
            &if s.host.is_none() {
                tr!(
                    "AMONHEN_GIT in the app’s environment overrides it: {program}",
                    program = git.program
                )
            } else {
                tr!(
                    "AMONHEN_GIT in the engine’s environment overrides it: {program}",
                    program = git.program
                )
            },
            &["settings-warning", "caption"],
        );
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_say_where_a_program_comes_from() {
        let c = |path: &str, version: Option<&str>| Candidate {
            path: path.into(),
            version: version.map(str::to_owned),
        };
        assert_eq!(
            c("/home/linuxbrew/.linuxbrew/bin/zz-no-such", Some("2.56.0")).label(false),
            "Homebrew 2.56.0"
        );
        assert_eq!(
            c("/opt/zz/bin/git", Some("2.54.0 (Apple Git-157)")).label(false),
            "/opt/zz/bin 2.54.0"
        );
        assert_eq!(c("/usr/bin/git", None).label(true), "/usr/bin");
        assert_eq!(
            c(r"C:\Program Files\Git\cmd\git.exe", Some("2.45.1")).label(true),
            r"C:\Program Files\Git\cmd 2.45.1"
        );
    }
}
