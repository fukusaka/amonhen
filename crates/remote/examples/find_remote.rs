//! SSH 先のスキャンを手で試す: `cargo run -p amonhen-remote --example find_remote -- ssh://host/~/prj [depth]`。
//! 送り込むエンジンは `dist/engine`（`scripts/build-engine.sh` で作る）。
fn main() -> amonhen_model::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let location = args.first().ok_or("ssh://host/path を渡してください")?;
    let depth = args.get(1).and_then(|d| d.parse().ok()).unwrap_or(3);
    let daemons = amonhen_remote::ssh::Daemons::new("dist/engine".into(), None);
    let excludes: Vec<String> = amonhen_model::scan::DEFAULT_EXCLUDES
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let target = amonhen_remote::ssh::Target::parse(location)?;
    let started = std::time::Instant::now();
    let found = amonhen_remote::ssh::find_repositories(
        "ssh",
        &target,
        &daemons,
        &Default::default(),
        depth,
        &excludes,
    )?;
    for path in &found {
        println!("{path}");
    }
    eprintln!("{} 件、{:?}", found.len(), started.elapsed());
    Ok(())
}
