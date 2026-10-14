//! 起点が変わったときの読み直しの時間を測る。`<repo>` にコミットを 1 つ足すので、使い捨ての
//! 複製（`git clone --shared` など）で実行すること。
//! `cargo run --release -p amonhen-core --example incremental -- <repo>`

use std::process::Command;
use std::time::Instant;

use amonhen_backend_gix::GixBackend;
use amonhen_core::Repository;

fn git(repo: &str, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn main() -> amonhen_model::Result<()> {
    let path = std::env::args().nth(1).expect("リポジトリを指定する");
    let repo = Repository::new(GixBackend::open(&path)?);

    let t = Instant::now();
    let first = repo.load_history()?;
    println!("初回: {} コミット {:?}", first.graph.len(), t.elapsed());

    let t = Instant::now();
    repo.load_history()?;
    println!("変化なし: {:?}", t.elapsed());

    let head = git(&path, &["rev-parse", "HEAD"]);
    let tree = git(&path, &["rev-parse", "HEAD^{tree}"]);
    let commit = git(
        &path,
        &["commit-tree", &tree, "-p", &head, "-m", "amonhen bench"],
    );
    git(&path, &["update-ref", "HEAD", &commit]);

    let t = Instant::now();
    let second = repo.load_history()?;
    println!(
        "1 コミット追加: {} コミット {:?}",
        second.graph.len(),
        t.elapsed()
    );
    Ok(())
}
