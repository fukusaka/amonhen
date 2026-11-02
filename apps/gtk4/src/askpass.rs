//! git と ssh が資格情報（HTTPS のユーザー名とパスワード、SSH の鍵のパスフレーズ、初めてつなぐホストの鍵の確認）を
//! 尋ねる先（macOS 版の `amonhen-askpass`）。アプリは端末なしで git を動かすので、尋ねられると失敗する。
//! 起動のときに、この実行ファイルを `--askpass` で呼ぶ小さなスクリプトを置いて `AMONHEN_ASKPASS` にし、コアが git を
//! 動かすときに `GIT_ASKPASS`・`SSH_ASKPASS` にする（共有の daemon にも引き継がれる）。`--askpass` では、プロンプトを
//! 小さな窓で尋ね、答えを標準出力に出す。キャンセルなら 1 で終わる（git は認証に失敗したことにする）。

use std::io::Write;
use std::path::PathBuf;

use amonhen_model::tr;
use gtk::glib;
use gtk::prelude::*;

/// `AMONHEN_ASKPASS` を決める（もう決まっていれば、そのまま）。
pub fn install() {
    if std::env::var_os("AMONHEN_ASKPASS").is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    // 置き場所: 利用者ごとの実行時のフォルダ（なければキャッシュ）。
    let dir: PathBuf = glib::user_runtime_dir().join("amonhen");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let script = dir.join("amonhen-gtk-askpass");
    let quoted = exe.to_string_lossy().replace('\'', "'\\''");
    let text = format!("#!/bin/sh\nexec '{quoted}' --askpass \"$@\"\n");
    let written = std::fs::read_to_string(&script).ok().as_deref() == Some(text.as_str())
        || std::fs::write(&script, &text).is_ok();
    #[cfg(unix)]
    if written {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700));
    }
    if written {
        // まだほかのスレッドを動かしていない（起動の初め）ので、環境を変えてよい。
        unsafe { std::env::set_var("AMONHEN_ASKPASS", &script) };
    }
}

/// `--askpass <プロンプト>`: 尋ねて答えを出す。終わり方（0 は答えた、1 はキャンセル）を返す。
pub fn run(prompt: &str) -> i32 {
    if gtk::init().is_err() {
        return 1;
    }
    amonhen_model::i18n::set_default(amonhen_model::i18n::Lang::from_env());
    // パスワードとパスフレーズは伏せる。ユーザー名と、ホストの鍵の確認は見せる。
    let hidden = !(prompt.to_lowercase().contains("username") || prompt.contains("(yes/no"));
    let label = gtk::Label::new(Some(prompt.trim()));
    label.set_wrap(true);
    label.set_max_width_chars(60);
    label.set_xalign(0.0);
    let entry = gtk::Entry::builder()
        .visibility(!hidden)
        .activates_default(true)
        .build();
    let cancel = gtk::Button::with_label(&tr!("Cancel"));
    let ok = gtk::Button::with_label("OK");
    ok.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&ok);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.append(&label);
    content.append(&entry);
    content.append(&buttons);
    let window = gtk::Window::builder()
        .title("Amon Hen")
        .default_width(420)
        .resizable(false)
        .child(&content)
        .build();
    window.set_default_widget(Some(&ok));
    let main = glib::MainLoop::new(None, false);
    let answer: std::rc::Rc<std::cell::RefCell<Option<String>>> = Default::default();
    let (a, m, w, e) = (
        answer.clone(),
        main.clone(),
        window.downgrade(),
        entry.clone(),
    );
    ok.connect_clicked(move |_| {
        a.replace(Some(e.text().to_string()));
        if let Some(w) = w.upgrade() {
            w.destroy();
        }
        m.quit();
    });
    let (m, w) = (main.clone(), window.downgrade());
    cancel.connect_clicked(move |_| {
        if let Some(w) = w.upgrade() {
            w.destroy();
        }
        m.quit();
    });
    let m = main.clone();
    window.connect_close_request(move |_| {
        m.quit();
        glib::Propagation::Proceed
    });
    window.present();
    entry.grab_focus();
    main.run();
    match answer.take() {
        Some(text) => {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{text}");
            0
        }
        None => 1,
    }
}
