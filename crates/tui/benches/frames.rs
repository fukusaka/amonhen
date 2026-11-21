//! TUI の 1 フレーム（入力・届いた答え・描いて端末に送るまで）の速さ。`amonhen tui --measure-*` と同じ歩き方を
//! criterion で測る（docs/tui.md の T1b）。リポジトリは `git fast-import` で作る: 5,000 コミットの History、
//! 先頭のコミットに 3,000 行のファイル（差分のスクロール）、作業ツリーにそのファイルの変更（Working Copy の差分）。

use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use amonhen_front::InProcess;
use amonhen_tui::{Scroll, Step};
use criterion::{Criterion, criterion_group, criterion_main};

const COMMITS: usize = 5_000;
const LINES: usize = 3_000;
const SIZE: (u16, u16) = (200, 60);

fn git(dir: &Path, args: &[&str]) {
    let status = amonhen_model::process::command("git")
        .args([
            "-c",
            "user.name=Bench",
            "-c",
            "user.email=bench@example.com",
        ])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// ベンチのリポジトリを作る。
fn repository(dir: &Path) {
    git(dir, &["init", "-q"]);
    let mut stream = String::new();
    let big: String = (0..LINES)
        .map(|i| format!("    let value_{i} = compute({i}, \"line {i}\");\n"))
        .collect();
    for i in 0..COMMITS {
        let time = 1_700_000_000 + i * 600;
        let (path, data) = match i + 1 == COMMITS {
            true => ("src/big.rs".to_owned(), big.clone()),
            false => (format!("src/file{}.rs", i % 50), format!("// change {i}\n")),
        };
        let message = format!("Change {i}\n");
        stream += "commit refs/heads/main\n";
        stream += &format!("committer Bench <bench@example.com> {time} +0000\n");
        stream += &format!("data {}\n{message}", message.len());
        // 同じブランチへ続けて書けば、前のコミットが親になる。
        stream += &format!("M 100644 inline {path}\ndata {}\n{data}\n", data.len());
    }
    let mut child = amonhen_model::process::command("git")
        .args(["fast-import", "--quiet"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    git(dir, &["checkout", "-q", "-f", "main"]);
    // 作業ツリーで、大きなファイルの 1 行おきを書き換える（Working Copy の差分）。
    let changed: String = big
        .lines()
        .enumerate()
        .map(|(i, line)| match i % 2 {
            0 => format!("{line} // edited\n"),
            _ => format!("{line}\n"),
        })
        .collect();
    std::fs::write(dir.join("src/big.rs"), changed).unwrap();
}

fn bench(c: &mut Criterion) {
    amonhen_model::testing::isolate_from_user_settings();
    let dir = tempfile::tempdir().unwrap();
    repository(dir.path());
    let steps = [
        ("history wheel", Step::Wheel),
        ("history move", Step::Move),
        ("commit diff scroll", Step::Diff),
        ("working copy diff move", Step::Changes),
        ("commit diff pan", Step::DiffPan),
    ];
    for (name, step) in steps {
        let front = Arc::new(InProcess::open(dir.path(), None).unwrap());
        let mut scroll = Scroll::new(front, SIZE.0, SIZE.1, step).unwrap();
        c.bench_function(&format!("{name} {}x{}", SIZE.0, SIZE.1), |b| {
            b.iter(|| scroll.step().unwrap())
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
