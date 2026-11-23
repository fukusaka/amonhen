//! Mermaid の図を SVG に描く。Obsidian の Mermaid の描き手 pele 1.13.0（https://github.com/obsidianmd/pele）を
//! TypeScript から Rust に移したもの。今は共通の部分と flowchart だけ。出力は pele 1.13.0 とバイト単位で同じに
//! する（文字の幅は canvas のない pele と同じく、Arial の送り幅の表から見積もる）。
//!
//! 構成は TS のファイルごとのモジュールに合わせてある。pele の新しい版や、ほかの図の種類も同じように移せる。
//! JS の数と文字列のふるまいのうち出力に届くもの（`Number.prototype.toString`、`Math.round`、UTF-16 の位置、
//! Map の入れた順、安定な並べ替え）は `js` と各所で写す。
//!
//! pele のライセンス:
//!
//! ```text
//! MIT License
//!
//! Copyright (c) 2026 kepano
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//! ```
//!
//! pele の LICENSE にある Lucide Icons の告知は、ズームの操作のアイコン（ブラウザ向けの部分）のもので、ここには
//! 移していない。

pub mod js;
pub mod layout;
