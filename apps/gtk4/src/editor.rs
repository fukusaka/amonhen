//! 外部のエディタ（macOS 版の `ExternalEditor`）。アプリのどこから「エディタで開く」ときも、Settings > General で
//! 選んだものを使う。選んでいなければ、入っているコードエディタのうち最初のもの、それもなければテキストエディタ
//! （テキストの既定のアプリか、テキストを開けるアプリのうち、ワープロなどのオフィスのアプリでないもの）。
//! 設定（`gtk.ini` の `externalEditor`）には、アプリの desktop の ID（`code.desktop` など）か、Other… で選んだ
//! 実行ファイルのパスを置く。

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// 設定の鍵（macOS 版の UserDefaults と同じ）。
pub const KEY: &str = "externalEditor";

/// 知っているコードエディタ（この順に探す。macOS 版の `known` と同じ順。desktop の ID と名前。Flatpak・Snap の
/// ID も）。
const KNOWN: &[(&str, &str)] = &[
    ("code.desktop", "VS Code"),
    ("com.visualstudio.code.desktop", "VS Code"),
    ("code_code.desktop", "VS Code"),
    ("code-insiders.desktop", "VS Code Insiders"),
    ("code-insiders_code-insiders.desktop", "VS Code Insiders"),
    ("cursor.desktop", "Cursor"),
    ("dev.zed.Zed.desktop", "Zed"),
    ("zed.desktop", "Zed"),
    ("sublime_text.desktop", "Sublime Text"),
    ("com.sublimetext.three.desktop", "Sublime Text"),
    ("sublime-text_subl.desktop", "Sublime Text"),
    ("codium.desktop", "VSCodium"),
    ("com.vscodium.codium.desktop", "VSCodium"),
    ("codium_codium.desktop", "VSCodium"),
    ("jetbrains-fleet.desktop", "Fleet"),
    ("org.kde.kate.desktop", "Kate"),
    ("org.gnome.Builder.desktop", "Builder"),
];

/// エディタのアプリ。
#[derive(Clone)]
pub struct App {
    /// 設定に置く値（desktop の ID か、実行ファイルのパス）。
    pub id: String,
    pub name: String,
    info: gio::AppInfo,
}

/// 入っているコードエディタ（KNOWN の順）。最後にテキストエディタ（macOS 版の既定のテキストエディタ）。
pub fn installed() -> Vec<App> {
    let all = gio::AppInfo::all();
    let mut apps: Vec<App> = Vec::new();
    for (id, name) in KNOWN {
        if let Some(info) = all.iter().find(|a| a.id().as_deref() == Some(*id))
            && !apps.iter().any(|a| a.name == *name)
        {
            apps.push(App {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                info: info.clone(),
            });
        }
    }
    if let Some(info) = text_editor()
        && let Some(id) = info.id()
        && !apps.iter().any(|a| a.id == id.as_str())
    {
        apps.push(App {
            id: id.to_string(),
            name: info.display_name().to_string(),
            info,
        });
    }
    apps
}

/// テキストエディタ: テキストの既定のアプリ、次にテキストを開けるアプリの順で、テキストエディタのもの。ワープロ
/// などのオフィスのアプリ（Ubuntu ではテキストの既定が LibreOffice Writer のことがある）は選ばない。
fn text_editor() -> Option<gio::AppInfo> {
    gio::AppInfo::default_for_type("text/plain", false)
        .into_iter()
        .chain(gio::AppInfo::all_for_type("text/plain"))
        .find(|info| {
            let id = info.id().map(|id| id.to_string()).unwrap_or_default();
            is_text_editor(&id, categories(&id).as_deref())
        })
}

/// desktop の ID のファイルの `Categories`（XDG のデータの場所の `applications` から探す）。desktop のファイルの
/// ない OS や、見つからなければ None。
fn categories(id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    std::iter::once(glib::user_data_dir())
        .chain(glib::system_data_dirs())
        .map(|dir| dir.join("applications").join(id))
        .find(|path| path.is_file())
        .and_then(|path| {
            let file = glib::KeyFile::new();
            file.load_from_file(&path, glib::KeyFileFlags::NONE).ok()?;
            Some(
                file.string("Desktop Entry", "Categories")
                    .map_or_else(|_| String::new(), |c| c.to_string()),
            )
        })
}

/// テキストエディタとして使えるか。オフィスのアプリは使わない。`categories` が分かれば `TextEditor` のものだけ。
/// 分からなければ（desktop のファイルのない OS）、オフィスのアプリでなければ使う。
fn is_text_editor(id: &str, categories: Option<&str>) -> bool {
    const OFFICE_IDS: [&str; 6] = [
        "libreoffice",
        "openoffice",
        "onlyoffice",
        "abiword",
        "calligra",
        "wps-office",
    ];
    const OFFICE_CATEGORIES: [&str; 4] = ["Office", "WordProcessor", "Spreadsheet", "Presentation"];
    let lower = id.to_lowercase();
    if OFFICE_IDS.iter().any(|o| lower.contains(o)) {
        return false;
    }
    match categories {
        Some(c) => {
            let c: Vec<&str> = c.split(';').map(str::trim).collect();
            !c.iter().any(|x| OFFICE_CATEGORIES.contains(x)) && c.contains(&"TextEditor")
        }
        None => true,
    }
}

/// 自動で選ばれるもの（入っているコードエディタの先頭）。
pub fn automatic() -> Option<App> {
    installed().into_iter().next()
}

/// 設定の値のアプリ（desktop の ID か、実行ファイルのパス）。なければ None。
pub fn app_of(value: &str) -> Option<App> {
    if value.is_empty() {
        return None;
    }
    // 実行ファイルのパス（Windows の `C:\…` も）。そうでなければアプリの ID。
    let path = std::path::Path::new(value);
    if path.is_absolute() {
        if !path.is_file() {
            return None;
        }
        let info = gio::AppInfo::create_from_commandline(
            path,
            None,
            gio::AppInfoCreateFlags::SUPPORTS_URIS,
        )
        .ok()?;
        return Some(App {
            id: value.to_owned(),
            name: display_name(value),
            info,
        });
    }
    let info = gio::AppInfo::all()
        .into_iter()
        .find(|a| a.id().as_deref() == Some(value))?;
    let name = KNOWN
        .iter()
        .find(|(id, _)| *id == value)
        .map_or_else(|| info.display_name().to_string(), |(_, n)| (*n).to_owned());
    Some(App {
        id: value.to_owned(),
        name,
        info,
    })
}

/// Settings で選んだもの。なければ自動。
pub fn chosen() -> Option<App> {
    crate::settings::string(KEY)
        .and_then(|v| app_of(&v))
        .or_else(automatic)
}

/// 選んだエディタの名前（なければ「Editor」）。
pub fn chosen_name() -> String {
    chosen().map_or_else(|| tr!("Editor"), |a| a.name)
}

/// 実行ファイルのパスの、見せる名前（ファイル名）。
pub fn display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// Open in Editor のツールチップ（どのエディタで開くか）。
pub fn tooltip() -> String {
    tr!("Opens in {app}", app = chosen_name())
}

/// フォルダ（リポジトリ・worktree）を選んだエディタで開く。開けなければ、そのわけ（macOS 版の `openFolder`）。
/// 知っているコードエディタはどれもフォルダを開ける。Other… で選んだ実行ファイルには、そのままパスを渡す。ほか
/// （テキストエディタ）は、フォルダを開けると宣言しているもの（`inode/directory`）だけ。
pub fn open_folder(dir: &std::path::Path) -> Result<(), String> {
    let Some(app) = chosen() else {
        return Err(tr!(
            "No editor is installed. Choose one in Settings > General."
        ));
    };
    let folder = gio::content_type_from_mime_type("inode/directory");
    let opens_folders = KNOWN.iter().any(|(id, _)| *id == app.id)
        || std::path::Path::new(&app.id).is_absolute()
        || gio::AppInfo::all_for_type(folder.as_deref().unwrap_or("inode/directory"))
            .iter()
            .any(|a| a.id().as_deref() == Some(app.id.as_str()));
    if !opens_folders {
        return Err(tr!(
            "{app} can’t open folders. Choose a code editor in Settings > General.",
            app = app.name
        ));
    }
    let context = gdk::Display::default().map(|d| d.app_launch_context());
    app.info
        .launch(&[gio::File::for_path(dir)], context.as_ref())
        .map_err(|e| e.to_string())
}

/// [`open_folder`] で開けなかったわけを、`parent` の上の知らせで。
pub fn open_folder_in(parent: Option<&gtk::Window>, dir: &std::path::Path) {
    if let Err(message) = open_folder(dir) {
        let parent = parent.cloned();
        glib::spawn_future_local(async move {
            crate::sheets::alert(parent.as_ref(), &tr!("Couldn’t Open in Editor"), &message).await;
        });
    }
}

/// ファイルをエディタで開く（`app` がなければ選んだもの。それもなければ、そのファイルの既定のアプリ）。
pub fn open(files: &[std::path::PathBuf], app: Option<App>, parent: Option<&gtk::Window>) {
    let files: Vec<gio::File> = files.iter().map(gio::File::for_path).collect();
    if let Some(app) = app.or_else(chosen) {
        let context = gdk::Display::default().map(|d| d.app_launch_context());
        if let Err(e) = app.info.launch(&files, context.as_ref()) {
            eprintln!("エディタで開けない: {e}");
        }
        return;
    }
    for file in files {
        let launcher = gtk::FileLauncher::new(Some(&file));
        let parent = parent.cloned();
        glib::spawn_future_local(async move {
            if let Err(e) = launcher.launch_future(parent.as_ref()).await {
                eprintln!("開けない: {e}");
            }
        });
    }
}

/// Settings > General の「External Editor」の行: 自動（その名前）・入っているもの・選んだ実行ファイル・Other…。
pub fn settings_row() -> gtk::Box {
    let dropdown = gtk::DropDown::from_strings(&[]);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = gtk::Label::new(Some(&tr!("External Editor")));
    l.set_hexpand(true);
    l.set_xalign(0.0);
    row.append(&l);
    row.append(&dropdown);

    // 項目の値（"" は自動、"\u{1}other" は Other…）。
    let values = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    // 区切りの線（macOS 版と同じく、自動の下と Other… の上）。
    let separators = std::rc::Rc::new(std::cell::RefCell::new(Vec::<u32>::new()));
    crate::settings::separate_items(&dropdown, separators.clone());
    const OTHER: &str = "\u{1}other";
    let fill = {
        let (dropdown, values) = (dropdown.clone(), values.clone());
        move || {
            let current = crate::settings::string(KEY).unwrap_or_default();
            let installed = installed();
            let mut items = vec![(
                String::new(),
                installed.first().map_or_else(
                    || tr!("Automatic"),
                    |a| tr!("Automatic ({label})", label = a.name),
                ),
            )];
            items.extend(installed.iter().map(|a| (a.id.clone(), a.name.clone())));
            if !current.is_empty() && !installed.iter().any(|a| a.id == current) {
                let name = app_of(&current).map_or_else(|| display_name(&current), |a| a.name);
                items.push((current.clone(), name));
            }
            items.push((OTHER.to_owned(), tr!("Other…")));
            separators.replace(separators_for(items.len()));
            let titles: Vec<&str> = items.iter().map(|(_, t)| t.as_str()).collect();
            dropdown.set_model(Some(&gtk::StringList::new(&titles)));
            let at = items.iter().position(|(v, _)| *v == current).unwrap_or(0);
            values.replace(items.into_iter().map(|(v, _)| v).collect());
            dropdown.set_selected(at as u32);
        }
    };
    fill();
    let filling = std::rc::Rc::new(std::cell::Cell::new(false));
    let fill = std::rc::Rc::new(fill);
    let (v, f, refill) = (values.clone(), filling.clone(), fill.clone());
    dropdown.connect_selected_notify(move |d| {
        if f.get() {
            return;
        }
        let Some(value) = v.borrow().get(d.selected() as usize).cloned() else {
            return;
        };
        if value != OTHER {
            crate::settings::set_string(KEY, &value);
            return;
        }
        // 選んでいる間も、今の値を出しておく（選んだら変える）。選んだ通知の中で model を替えると GTK の一覧が
        // 固まるので、通知が終わってから替える。
        let (f2, fill2) = (f.clone(), refill.clone());
        glib::idle_add_local_once(move || {
            f2.set(true);
            fill2();
            f2.set(false);
        });
        let (f, fill) = (f.clone(), refill.clone());
        let parent = d.root().and_downcast::<gtk::Window>();
        // この機械のプログラムを選ぶシート（macOS 版と同じく、アプリのファイルを選ぶ形。Settings > Git > Programs と同じ）。
        let current = crate::settings::string(KEY).unwrap_or_default();
        let initial = if std::path::Path::new(&current).is_absolute() {
            current
        } else if cfg!(unix) {
            "/usr/bin".to_owned()
        } else {
            String::new()
        };
        let request = crate::filepicker::Request {
            title: tr!("External Editor"),
            message: tr!("Choose the app to open files in."),
            kind: crate::filepicker::Kind::Executable,
            initial,
            places: crate::filepicker::local_program_places(),
            ..Default::default()
        };
        glib::spawn_future_local(async move {
            if let Some(chosen) = crate::filepicker::choose(
                parent.as_ref(),
                request,
                crate::filepicker::Source::Local,
            )
            .await
            {
                crate::settings::set_string(KEY, &chosen.path);
            }
            f.set(true);
            fill();
            f.set(false);
        });
    });
    // タブを出すたびに入っているエディタを読み直す（あとで入れたものも出る。macOS 版の onAppear と同じ）。
    // 選んでいる最中に model を替えないよう、選んだ通知と同じ印を立てる。
    row.connect_map(move |_| {
        filling.set(true);
        fill();
        filling.set(false);
    });
    row
}

/// 項目が `count` 個（自動・入っているもの…・Other…）のときの、線を引く位置（自動の下と、Other… の上）。
fn separators_for(count: usize) -> Vec<u32> {
    let other = count.saturating_sub(1) as u32;
    if other > 1 {
        vec![1, other]
    } else {
        vec![other]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_apps_are_never_the_text_editor() {
        assert!(!is_text_editor(
            "libreoffice-writer.desktop",
            Some("Office;WordProcessor;X-Red-Hat-Base;")
        ));
        assert!(!is_text_editor(
            "org.libreoffice.LibreOffice.writer.desktop",
            None
        ));
        assert!(!is_text_editor(
            "abiword.desktop",
            Some("TextEditor;Office;")
        ));
        assert!(is_text_editor(
            "org.gnome.TextEditor.desktop",
            Some("GNOME;GTK;Utility;TextEditor;")
        ));
        assert!(!is_text_editor(
            "org.gnome.Nautilus.desktop",
            Some("GNOME;Utility;Core;")
        ));
        // desktop のファイルのない OS（Windows のメモ帳など）。
        assert!(is_text_editor("notepad.exe", None));
    }

    #[test]
    fn separators_sit_below_automatic_and_above_other() {
        assert_eq!(separators_for(2), [1]);
        assert_eq!(separators_for(4), [1, 3]);
    }
}
