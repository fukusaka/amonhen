fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=../layout/src");
    let config = cbindgen::Config::from_file(format!("{dir}/cbindgen.toml")).unwrap();
    cbindgen::generate_with_config(&dir, config)
        .expect("ヘッダーを生成できない")
        .write_to_file(format!("{dir}/include/amonhen.h"));
}
