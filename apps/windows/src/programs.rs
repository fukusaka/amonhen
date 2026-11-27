//! Settings > Git > Programs（GTK4 版の `programs.rs`、macOS 版の `MachineSettings.swift` の `ProgramsPage`）:
//! git・gpg・ssh-keygen・gpgsm を同じ形で選ぶ（自動＝PATH の先頭／見つかったもの／その他…）。git はこの機械の
//! Amon Hen の設定（`settings.json`。CLI・daemon・VSCode 拡張も従う。コアの `machineGit`・`setMachineGit`）、
//! ほかはすべてのリポジトリの段の git の設定（`gpg.program` など）。候補はコアの `programs` で探す。

use amonhen_model::tr;
use serde_json::{Value, json};
use windows_reactor::*;

/// 選ぶ欄の id（行ごと）と、探すプログラムの名前。
pub const ROWS: [(&str, &str); 4] = [
    ("program:git", "git"),
    ("program:gpg", "gpg"),
    ("program:ssh-keygen", "ssh-keygen"),
    ("program:gpgsm", "gpgsm"),
];

/// プログラムを決める git の設定のキー（先に決まっているものを使い、どれもなければ最後のキーに書く）。
fn config_keys(name: &str) -> &'static [&'static str] {
    match name {
        "gpg" => &["gpg.openpgp.program", "gpg.program"],
        "ssh-keygen" => &["gpg.ssh.program"],
        "gpgsm" => &["gpg.x509.program"],
        _ => &[],
    }
}

/// プログラムの候補（コアの `programs`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub path: String,
    pub version: Option<String>,
}

impl Candidate {
    /// フォルダと版（`C:\Program Files\Git\cmd 2.55.0`）。
    fn label(&self) -> String {
        let folder = match self.path.rfind(['/', '\\']) {
            Some(i) if i > 0 => self.path[..i].to_owned(),
            _ => self.path.clone(),
        };
        match self
            .version
            .as_deref()
            .and_then(|v| v.split_whitespace().next())
        {
            Some(v) => format!("{folder} {v}"),
            None => folder,
        }
    }
}

/// コアの `programs` の答え（info）から。
pub fn candidates(info: &Value) -> Vec<Candidate> {
    info["programs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Candidate {
                path: p["path"].as_str()?.to_owned(),
                version: p["version"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

/// 使う git と、それをどこで決めたか（コアの `machineGit`）。
#[derive(Clone, Debug, Default)]
pub struct MachineGit {
    pub chosen: Option<String>,
    /// Amon Hen の設定のファイル（ⓘ に出す）。
    pub file: Option<String>,
    pub program: String,
    pub source: String,
    pub problem: Option<String>,
}

impl MachineGit {
    pub fn from_info(info: &Value) -> Self {
        let s = |k: &str| info[k].as_str().map(str::to_owned);
        Self {
            chosen: s("chosen"),
            file: s("file"),
            program: s("program").unwrap_or_default(),
            source: s("source").unwrap_or_default(),
            problem: s("problem"),
        }
    }

    fn source_label(&self) -> &'static str {
        match self.source.as_str() {
            "settings" => "Amon Hen",
            "environment" => "AMONHEN_GIT",
            "fallback" => "AMONHEN_FALLBACK_GIT",
            _ => "PATH",
        }
    }
}

/// 探した候補と使う git（読むまでは None）。
#[derive(Default)]
pub struct State {
    pub found: Vec<(&'static str, Vec<Candidate>)>,
    pub git: Option<MachineGit>,
    /// その他… を選んだ回数（取りやめたとき、欄を今の値に戻すため作り直す）。
    pub epoch: u32,
}

/// 候補と使う git を読む（裏で呼ぶ。版を読むのにプログラムを動かす）。
pub fn load(run: impl Fn(&str) -> String) -> State {
    let found = ROWS
        .iter()
        .map(|(_, name)| {
            let reply: Value =
                serde_json::from_str(&run(&json!({ "op": "programs", "name": name }).to_string()))
                    .unwrap_or_default();
            (*name, candidates(&reply["info"]))
        })
        .collect();
    let reply: Value = serde_json::from_str(&run(r#"{"op":"machineGit"}"#)).unwrap_or_default();
    State {
        found,
        git: Some(MachineGit::from_info(&reply["info"])),
        epoch: 0,
    }
}

impl State {
    fn found(&self, name: &str) -> &[Candidate] {
        self.found
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(&[], |(_, c)| c.as_slice())
    }

    /// `name` の今の値（自動なら空）。git は Amon Hen の設定、ほかはすべてのリポジトリの段。
    fn current(&self, name: &str, global: &crate::reposettings::State) -> String {
        if name == "git" {
            return self
                .git
                .as_ref()
                .and_then(|g| g.chosen.clone())
                .unwrap_or_default();
        }
        global
            .at("global", key_of(name, global))
            .unwrap_or_default()
            .to_owned()
    }

    /// 選ぶ欄の項目（値と見出し）: 自動、見つかったもの、今の値（候補になければ）、その他…。
    fn options(&self, name: &str, current: &str) -> Vec<(String, String)> {
        let found = self.found(name);
        let automatic = match found.first() {
            Some(c) => tr!("Automatic ({label})", label = c.label()),
            None => tr!("Automatic"),
        };
        let mut items = vec![(String::new(), automatic)];
        items.extend(found.iter().map(|c| (c.path.clone(), c.label())));
        if !current.is_empty() && !found.iter().any(|c| c.path == current) {
            items.push((current.to_owned(), current.to_owned()));
        }
        items.push((OTHER.to_owned(), tr!("Other…")));
        items
    }
}

/// 「その他…」の項目の値（パスと混ざらないもの）。
const OTHER: &str = "\u{1}other";

/// 書くキー（決めてあるキーを先に。どれもなければ最後のキー）。
fn key_of(name: &str, global: &crate::reposettings::State) -> &'static str {
    let keys = config_keys(name);
    keys.iter()
        .copied()
        .find(|k| global.effective(k).is_some())
        .unwrap_or(keys[keys.len() - 1])
}

/// 選んだ結果。
pub enum Choice {
    /// この命令を書く（git を選んだなら、後で使う git を読み直す）。
    Write(Vec<Value>, bool),
    /// 実行ファイルを選んでもらう（プログラムの名前）。
    Pick(&'static str),
}

/// 選ぶ欄で `id` の行の `index` 番目を選んだ。今と同じなら None。
pub fn choose(
    state: &mut State,
    global: &crate::reposettings::State,
    id: &str,
    index: usize,
) -> Option<Choice> {
    let (_, name) = ROWS.iter().find(|(i, _)| *i == id)?;
    let current = state.current(name, global);
    let (value, _) = state.options(name, &current).get(index)?.clone();
    if value == OTHER {
        state.epoch += 1;
        return Some(Choice::Pick(name));
    }
    (value != current).then(|| set(name, &value, global))
}

/// `name` を `value`（空なら自動）にする。
pub fn set(name: &str, value: &str, global: &crate::reposettings::State) -> Choice {
    let value = (!value.is_empty()).then_some(value);
    if name == "git" {
        return Choice::Write(vec![json!({ "op": "setMachineGit", "git": value })], true);
    }
    Choice::Write(
        vec![json!({
            "op": "configSet", "key": key_of(name, global), "value": value, "scope": "global",
        })],
        false,
    )
}

/// ⓘ に出す、どこで何に決めているか（GTK4 版の `git_details`、git の設定は段ごとの値）。
fn details(state: &State, name: &str, global: &crate::reposettings::State) -> Vec<String> {
    if name == "git" {
        let Some(git) = state.git.as_ref() else {
            return Vec::new();
        };
        let mut lines: Vec<String> = git.file.iter().cloned().collect();
        lines.push(match &git.chosen {
            Some(c) => format!("\"git\": \"{c}\""),
            None => tr!("No git chosen: the first git on the PATH"),
        });
        return lines;
    }
    config_keys(name)
        .iter()
        .flat_map(|k| {
            global
                .entries
                .iter()
                .flatten()
                .filter(|e| e.key.eq_ignore_ascii_case(k))
                .map(|e| {
                    format!(
                        "{}: {} = {}",
                        crate::reposettings::scope_label(&e.scope),
                        e.key,
                        e.value
                    )
                })
        })
        .collect()
}

/// Programs の行（読むまでは「読み込み中…」）。`revert` は ↩︎（自動に戻す）。
pub fn view(
    state: Option<&State>,
    global: &crate::reposettings::State,
    choice: &dyn Fn(&'static str) -> Callback<Option<usize>>,
    revert: &dyn Fn(&'static str) -> Callback<()>,
) -> View {
    let Some(state) = state else {
        return TextBlock::new().text(tr!("Loading…")).opacity(0.6).into();
    };
    let rows: Vec<(String, View)> = ROWS
        .iter()
        .map(|&(id, name)| {
            let current = state.current(name, global);
            let options = state.options(name, &current);
            let selected = options.iter().position(|(v, _)| *v == current).unwrap_or(0);
            let path = if current.is_empty() {
                state
                    .found(name)
                    .first()
                    .map(|c| c.path.clone())
                    .unwrap_or_default()
            } else {
                current.clone()
            };
            let source = if name == "git" {
                state
                    .git
                    .as_ref()
                    .map_or("PATH", MachineGit::source_label)
                    .to_owned()
            } else {
                config_keys(name)
                    .iter()
                    .find_map(|k| global.effective(k))
                    .map(|(_, scope)| crate::reposettings::scope_label(scope))
                    .unwrap_or_else(|| tr!("git’s default"))
            };
            let problem = (name == "git")
                .then(|| state.git.as_ref().and_then(|g| g.problem.clone()))
                .flatten();
            let overridden = (name == "git")
                .then_some(state.git.as_ref())
                .flatten()
                .filter(|g| g.source == "environment")
                .map(|g| {
                    tr!(
                        "AMONHEN_GIT in the app’s environment overrides it: {program}",
                        program = g.program.as_str()
                    )
                });
            // ↩︎: 選んでいれば、やめて自動に戻す（git の設定は上の段、なければ git の既定。リポジトリの
            // 設定の行と同じ）。
            let revert_button: View = if current.is_empty() {
                View::empty()
            } else {
                crate::small::button("↩︎", |b| b.on_click(revert(id)))
                    .tooltip(tr!("Stop overriding here. The value above takes over."))
            };
            // ⓘ: どこで何に決めているか（マウスを載せると出す）。
            let lines = details(state, name, global);
            let info: View = if lines.is_empty() {
                View::empty()
            } else {
                TextBlock::new()
                    .text("ⓘ")
                    .opacity(0.6)
                    .vertical_alignment(VerticalAlignment::Center)
                    .tooltip(lines.join("\n"))
            };
            let note = problem
                .map(|p| format!("⚠ {p}"))
                .or(overridden)
                .unwrap_or_default();
            (
                format!("{id}:{}:{current}:{}", state.epoch, options.len()),
                StackPanel::new().spacing(2.0).children((
                    TextBlock::new().text(name.to_owned()),
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .children((
                            ComboBox::new()
                                .items_source(
                                    options.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>(),
                                )
                                .selected_index(selected)
                                .min_width(300.0)
                                .on_selection_changed(choice(id))
                                .tooltip(path),
                            revert_button,
                            TextBlock::new()
                                .text(source)
                                .font_size(12.0)
                                .opacity(0.6)
                                .vertical_alignment(VerticalAlignment::Center),
                            info,
                        )),
                    if note.is_empty() {
                        View::empty()
                    } else {
                        TextBlock::new()
                            .text(note)
                            .font_size(12.0)
                            .text_wrapping(TextWrapping::Wrap)
                            .into()
                    },
                )),
            )
        })
        .collect();
    StackPanel::new().spacing(10.0).children((
        View::keyed_fragment(rows),
        TextBlock::new()
            .text(tr!(
                "The CLI, the shared daemon and the VSCode extension on this machine use it too."
            ))
            .font_size(12.0)
            .opacity(0.65)
            .text_wrapping(TextWrapping::Wrap),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choosing_writes_the_setting() {
        let mut s = State {
            found: vec![(
                "gpg",
                vec![Candidate {
                    path: r"C:\Program Files\GnuPG\bin\gpg.exe".into(),
                    version: Some("2.4.5".into()),
                }],
            )],
            git: Some(MachineGit::default()),
            epoch: 0,
        };
        let global = crate::reposettings::State::new();
        // 自動、見つかったもの、その他…。
        let Some(Choice::Write(c, false)) = choose(&mut s, &global, "program:gpg", 1) else {
            panic!("書く");
        };
        assert_eq!(c[0]["key"], "gpg.program");
        assert_eq!(c[0]["scope"], "global");
        assert!(matches!(
            choose(&mut s, &global, "program:gpg", 2),
            Some(Choice::Pick("gpg"))
        ));
        // git は Amon Hen の設定。自動は今と同じなので書かない。
        assert!(choose(&mut s, &global, "program:git", 0).is_none());
        let Choice::Write(c, true) = set("git", r"C:\git\cmd\git.exe", &global) else {
            panic!("書く");
        };
        assert_eq!(c[0]["op"], "setMachineGit");
    }
}
