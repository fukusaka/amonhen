//! `amonhend`: daemon だけの実行ファイル（開発と試験用。配るのは CLI と 1 つにした `amonhen`）。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(amonhen_daemon::run(&args));
}
