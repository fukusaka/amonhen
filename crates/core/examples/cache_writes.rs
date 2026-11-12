//! History の永続キャッシュへの書き込みを、変化の種類ごとに測る。`<repo>` に ref やコミットを足すので、
//! 使い捨ての複製（`git clone --shared --no-checkout` など）で実行すること。
//! `cargo run --release -p amonhen-core --example cache_writes -- <repo> <cache-dir> [<detach-to>]`
//!
//! 変化ごとに、History を読み直す時間と、キャッシュに書いたもの（足した行・切った行・バイト数）と時間、キャッシュ
//! から読んで上の方だけレイアウトする時間（次に開くときの待ち）を出す。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;
use amonhen_core::cache;
use amonhen_core::graph::Layouter;
use amonhen_model::HistoryOrder;

fn git(repo: &str, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// `parent` の上に、同じツリーのコミットを 1 つ作る。
fn commit_on(repo: &str, parent: &str, message: &str) -> String {
    let tree = git(repo, &["rev-parse", &format!("{parent}^{{tree}}")]);
    git(repo, &["commit-tree", &tree, "-p", parent, "-m", message])
}

fn main() -> amonhen_model::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("リポジトリを指定する");
    let cache_dir = args.next().expect("キャッシュのフォルダを指定する");
    let detach_to = args.next();
    let file = Path::new(&cache_dir).join("history");
    cache::remove(&file)?;
    // 書き込みは自分で測るので、リポジトリにはキャッシュを持たせない。
    let repo = Repository::new(GixBackend::open(&path)?);

    let step = |name: &str, change: &mut dyn FnMut()| -> amonhen_model::Result<()> {
        change();
        let t = Instant::now();
        let h = repo.load_history()?;
        let load = t.elapsed();
        let t = Instant::now();
        let w = cache::write(
            &file,
            cache::Entry {
                tips: &h.tips,
                shallow: &h.shallow,
                head: h.head,
                order: HistoryOrder::Date,
                graph: &h.graph,
            },
        )?;
        let write = t.elapsed();
        let t = Instant::now();
        let read = cache::read(&file).expect("読めない");
        let mut l = Layouter::new(&read.graph, None);
        l.advance(10_000);
        let open = t.elapsed();
        println!(
            "{name}\t{} 行\t読み直し {load:?}\t足した {} 行・切った {} 行・{} バイト（{write:?}）\t開く {open:?}",
            h.graph.len(),
            w.rows,
            w.cut,
            w.bytes,
        );
        Ok(())
    };

    let p = path.as_str();
    step("開く（初回）", &mut || {})?;
    step("変化なし", &mut || {})?;
    step("コミット 1 つ", &mut || {
        let c = commit_on(p, "HEAD", "bench commit");
        git(p, &["update-ref", "HEAD", &c]);
    })?;
    step("amend", &mut || {
        let c = commit_on(p, "HEAD~1", "bench amend");
        git(p, &["update-ref", "HEAD", &c]);
    })?;
    step("rebase（5 コミット）", &mut || {
        let mut c = commit_on(p, "HEAD~5", "bench rebase base");
        for i in 0..5 {
            c = commit_on(p, &c, &format!("bench rebase {i}"));
        }
        git(p, &["update-ref", "HEAD", &c]);
    })?;
    step("reset（3 つ戻す）", &mut || {
        let c = git(p, &["rev-parse", "HEAD~3"]);
        git(p, &["update-ref", "HEAD", &c]);
    })?;
    step("既存のコミットにタグ", &mut || {
        git(p, &["update-ref", "refs/tags/bench-tag", "HEAD~10"]);
    })?;
    step("fetch（リモートに 3 コミット）", &mut || {
        let mut c = git(p, &["rev-parse", "HEAD"]);
        for i in 0..3 {
            c = commit_on(p, &c, &format!("bench upstream {i}"));
        }
        git(p, &["update-ref", "refs/remotes/bench/main", &c]);
    })?;
    step(
        "push の後（リモートが手元に揃う）",
        &mut || {
            git(p, &["update-ref", "refs/remotes/bench/main", "HEAD"]);
        },
    )?;
    step("ブランチを作ってコミット 3 つ", &mut || {
        let mut c = git(p, &["rev-parse", "HEAD~100"]);
        for i in 0..3 {
            c = commit_on(p, &c, &format!("bench topic {i}"));
        }
        git(p, &["update-ref", "refs/heads/bench-topic", &c]);
    })?;
    step("そのブランチを消す", &mut || {
        git(p, &["update-ref", "-d", "refs/heads/bench-topic"]);
    })?;
    if let Some(target) = &detach_to {
        step(&format!("{target} へ checkout（detach）"), &mut || {
            let c = git(p, &["rev-parse", &format!("{target}^{{commit}}")]);
            git(p, &["update-ref", "--no-deref", "HEAD", &c]);
        })?;
    }
    Ok(())
}
