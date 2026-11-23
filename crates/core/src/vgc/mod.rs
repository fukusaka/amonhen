//! flutter_svg の vector_graphics_compiler（vgc）を Rust に移したもの。SVG を vector_graphics のバイナリ（`.vec`）に
//! する。Dart がなくても、flutter_svg で SVG がどう描かれるかを見せられる（できた `.vec` は
//! `svgflutter::vec_to_svg` で SVG に戻す）。
//!
//! 移したもと: vector_graphics_compiler 1.3.6、vector_graphics_codec 1.2.0、path_parsing 1.1.0。vgc は flutter_svg と
//! 同じ設定（`--no-optimize-masks --no-optimize-clips --no-optimize-overdraw`、currentColor は黒、font-size は 14）で
//! 動かしたときと同じにする。最適化（path_ops）と三角形への分割（tessellator）は使わないので移していない。
//! 出力のバイト列と警告、失敗したときのエラー文（Dart の例外の `toString()`）が本物の vgc と同じになるように、
//! Dart の数の計算の順序（倍精度、書き出すときに単精度）や、Dart の `Map` の順序と `==` での重複のまとめ方まで
//! 同じにしている。後の版を追いやすいように、モジュールと関数の分け方は Dart のソースに合わせる:
//!
//! - `xml`: package:xml のイベントの読み方（vgc はイベントを先頭から読む）
//! - `parser`・`node`: `svg/parser.dart`・`svg/node.dart`（木と属性、id の参照）
//! - `parsers`・`colors`: `svg/parsers.dart`・`svg/numbers.dart`・`svg/colors.dart`
//! - `resolver`: `svg/resolver.dart`（参照を解いて描く木にする）
//! - `builder`: `svg/visitor.dart`・`draw_command_builder.dart`・`vector_graphics_compiler.dart` の
//!   `_encodeInstructions`・vector_graphics_codec
//! - `geometry`・`paint`: `geometry/*.dart`・`paint.dart`
//! - `path_parsing`: path_parsing
//! - `dart`: Dart の実行時のふるまい（`double.parse` など）
//!
//! vector_graphics_compiler と vector_graphics_codec のライセンス（path_parsing は MIT で、`path_parsing` に載せた）:
//!
//! ```text
//! Copyright 2013 The Flutter Authors
//!
//! Redistribution and use in source and binary forms, with or without modification,
//! are permitted provided that the following conditions are met:
//!
//!     * Redistributions of source code must retain the above copyright
//!       notice, this list of conditions and the following disclaimer.
//!     * Redistributions in binary form must reproduce the above
//!       copyright notice, this list of conditions and the following
//!       disclaimer in the documentation and/or other materials provided
//!       with the distribution.
//!     * Neither the name of Google Inc. nor the names of its
//!       contributors may be used to endorse or promote products derived
//!       from this software without specific prior written permission.
//!
//! THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
//! ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
//! WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//! DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR
//! ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
//! (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
//! LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
//! ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
//! (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
//! SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//! ```

mod builder;
mod colors;
mod dart;
mod geometry;
mod node;
mod paint;
mod parser;
mod parsers;
mod path_parsing;
mod resolver;
mod xml;

pub use dart::Error;

/// 移したもとの vector_graphics_compiler の版。
pub const VERSION: &str = "1.3.6";

/// 変換の結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    /// vector_graphics のバイナリ（`.vec`）。
    pub vec: Vec<u8>,
    /// vgc の警告（`Warning: Unsupported image format svg+xml` など、vgc の英語のまま）。
    pub warnings: Vec<String>,
}

/// SVG を `.vec` にする。vgc が失敗する SVG では、vgc と同じ文言のエラー（例外の `toString()`、各行の末尾の空白を
/// 落としたもの）を返す。
pub fn compile(svg: &[u8]) -> Result<Compiled, Error> {
    compile_inner(svg).map_err(|e| Error(normalize(&e.0)))
}

fn compile_inner(svg: &[u8]) -> dart::R<Compiled> {
    let Ok(xml) = std::str::from_utf8(svg) else {
        return Err(Error(
            "FileSystemException: Failed to decode data using encoding 'utf-8'".into(),
        ));
    };
    let (parsed, warnings) = parser::SvgParser::new(xml).parse()?;
    let tree = {
        let mut resolver = resolver::Resolver::new(&parsed.defs);
        resolver.visit(
            &node::Node::Parent(parsed.root.clone()),
            &geometry::AffineMatrix::IDENTITY,
        )?
    };
    let mut builder = builder::Builder::new();
    builder.visit(&tree);
    Ok(Compiled {
        vec: builder.encode(),
        warnings,
    })
}

/// vgc が出力に書いたエラーを読むときと同じに整える（各行の末尾の空白と、前後の空行を落とす）。
fn normalize(message: &str) -> String {
    let lines: Vec<&str> = message.lines().map(str::trim_end).collect();
    lines.join("\n").trim().to_owned()
}
