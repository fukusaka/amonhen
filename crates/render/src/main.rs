//! `amonhen-render`: VSCode 拡張の絵の手伝い。標準入力の JSON の行の求めに、標準出力の JSON の行で答える
//! （`amonhen_render::serve`。取り決めは docs/front.md の「The drawing helper」）。入力が尽きたら終わる。

fn main() {
    if matches!(std::env::args().nth(1).as_deref(), Some("-V" | "--version")) {
        println!("amonhen-render {}", amonhen_render::VERSION);
        return;
    }
    amonhen_model::i18n::set_default(amonhen_model::i18n::Lang::from_env());
    let stdin = std::io::stdin();
    amonhen_render::serve(std::io::BufReader::new(stdin), std::io::stdout());
}
