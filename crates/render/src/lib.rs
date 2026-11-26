//! VSCode 拡張の絵の手伝い（`amonhen-render`）。History の「すべてのファイル」の、Markdown の Mermaid・DOT の図と数式、
//! Android の VectorDrawable の絵、SVG のモバイル向けの確認を作る。
//!
//! 絵の機能（`amonhen-core` の `diagrams`・`math` のフィーチャー）は、エンジン（CLI・共有の daemon・SSH 先の
//! `--stdio`）には入れない。拡張はこの実行ファイルを 1 つのファイルを見せるたびに起動し、標準入力と標準出力で
//! JSON の行をやりとりする。リポジトリは読まないので、図の画像や VectorDrawable の色の資源など、同じコミットの
//! ほかのファイルが要るときは、それを求める行を書き、拡張が（エンジンから読んで）答える。取り決めは
//! `docs/front.md` の「The drawing helper」。

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use amonhen_core::diagram::{self, Drawn, ImageSource, MermaidConfig, Scheme, ThemeColors};
use amonhen_core::{math, svglint, svgmobile, vectordrawable};
use serde::Deserialize;
use serde_json::{Value, json};

/// 配る版（エンジンと同じく `scripts/build-engine.sh` が `AMONHEN_VERSION` で埋め込む）。
pub const VERSION: &str = match option_env!("AMONHEN_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// 求め（`op` で種類を分ける）。
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Request {
    /// Markdown のフェンスの図（`language` は ` ```mermaid ` などのフェンスの言葉）。`from` は Markdown のファイルの
    /// パス（DOT の図の画像を、そのフォルダからの相対パスで読む）。`scheme` は図の配色（アプリの設定「図の配色」の
    /// 鍵: `syntax`・`pele`・`mermaid`・`mermaid-forest`・`mermaid-import:名前` など。なければアプリと同じ既定の
    /// `mermaid`）、`importedTheme` は `scheme` が読み込んだテーマのときの、mermaid-cli の設定ファイルの中身。
    /// `timeZone` は gantt の日付を数える時間帯（空は手伝いの OS の時間帯、`UTC`、IANA の名前。読めない名前は OS の
    /// 時間帯。アプリと同じ）。`utcOffset` は前の取り決めの決まった時差（分、地方時 − UTC）で、`timeZone` が先。
    /// どちらもなければ UTC。
    Diagram {
        language: String,
        text: String,
        #[serde(default)]
        from: Option<String>,
        colors: Colors,
        #[serde(default)]
        scheme: Option<String>,
        #[serde(default)]
        imported_theme: Option<String>,
        #[serde(default)]
        time_zone: Option<String>,
        #[serde(default)]
        utc_offset: Option<i32>,
    },
    /// 1 つのファイルの数式（出てくる順。ファイルごとの上限はアプリと同じ）。`color` は文字の色。`timeLimit` は
    /// 1 つのファイルで式を描く時間の上限（秒。0 は上限なし。なければコアの既定の 2 秒）。
    Math {
        formulas: Vec<Formula>,
        color: String,
        #[serde(default)]
        time_limit: Option<u64>,
    },
    /// VectorDrawable のファイル `path`（中身 `data` は base64）。引けない色は `fallback` で描く。`dark` なら暗い外観
    /// の資源（`values-night` など）を先に引く。
    VectorDrawable {
        path: String,
        data: String,
        #[serde(default)]
        fallback: Option<String>,
        #[serde(default)]
        dark: bool,
    },
    /// SVG（`data` は base64）の、モバイル向けの描画部品で崩れる機能と、部品ごとの見え方（Android と flutter_svg は
    /// 本物の取り込みを移したもの、iOS と react-native-svg は近似。アプリと同じ）。
    SvgCheck { data: String },
}

/// 図に当てる画面の色（`#rrggbb`）。`kinds` は構文の種類（`keyword` など）→ 色で、ないものは `text`。
#[derive(Deserialize)]
struct Colors {
    #[serde(default)]
    dark: bool,
    text: String,
    background: String,
    #[serde(default)]
    kinds: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Formula {
    tex: String,
    #[serde(default)]
    display: bool,
}

/// 地方時との差が決まった時間帯（前の取り決めの、拡張が渡す時差。gantt の日付）。
struct FixedZone(f64);

impl diagram::TimeZone for FixedZone {
    fn offset_ms(&self, _utc_ms: f64) -> f64 {
        self.0
    }
}

/// IANA の時間帯（夏時間も数える。gantt の日付）。
struct NamedZone(jiff::tz::TimeZone);

impl diagram::TimeZone for NamedZone {
    fn offset_ms(&self, utc_ms: f64) -> f64 {
        if !utc_ms.is_finite() {
            return 0.0;
        }
        jiff::Timestamp::from_millisecond(utc_ms.floor() as i64)
            .map_or(0.0, |at| f64::from(self.0.to_offset(at).seconds()) * 1000.0)
    }
}

/// 設定の時間帯の名前（空・`system` は手伝いの OS の時間帯、読めない名前も OS の時間帯。アプリと同じ）。
fn named_zone(name: &str) -> jiff::tz::TimeZone {
    match name.trim() {
        "" | "system" => jiff::tz::TimeZone::system(),
        "UTC" => jiff::tz::TimeZone::UTC,
        name => jiff::tz::TimeZone::get(name).unwrap_or_else(|_| jiff::tz::TimeZone::system()),
    }
}

/// 拡張とのつながり: 答えの行を書き、同じコミットのファイルを求めて答えを待つ。
struct Link {
    out: Mutex<Box<dyn Write + Send>>,
    answers: Mutex<mpsc::Receiver<Value>>,
    next: AtomicU64,
}

impl Link {
    fn send(&self, value: &Value) {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        // 拡張が先に閉じたときは、書けなくても続ける（入力が尽きれば終わる）
        let _ = writeln!(out, "{value}").and_then(|()| out.flush());
    }

    /// 求めの行（`ask` に番号）を書いて、同じ番号の答えを待つ。入力が尽きたら None。
    fn ask(&self, mut question: Value) -> Option<Value> {
        let n = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        question["ask"] = n.into();
        self.send(&question);
        let answers = self.answers.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let answer = answers.recv().ok()?;
            if answer["answer"].as_u64() == Some(n) {
                return Some(answer);
            }
        }
    }

    /// 同じコミットのファイル（リポジトリの根からのパス）の中身。なければ None。
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let answer = self.ask(json!({ "read": path }))?;
        decode_base64(answer["data"].as_str()?)
    }

    /// 同じコミットのフォルダ（根は空）の中身（名前と、フォルダか）。
    fn list(&self, folder: &str) -> Vec<(String, bool)> {
        let Some(answer) = self.ask(json!({ "list": folder })) else {
            return Vec::new();
        };
        answer["entries"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| {
                        Some((
                            e["name"].as_str()?.to_owned(),
                            e["folder"].as_bool().unwrap_or(false),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// VectorDrawable の色の資源を、拡張に求めて読む（読んだフォルダは覚える）。
struct LinkFiles<'a> {
    link: &'a Link,
    folders: RefCell<HashMap<String, Vec<(String, bool)>>>,
}

impl vectordrawable::Files for LinkFiles<'_> {
    fn list(&self, folder: &str) -> Vec<(String, bool)> {
        self.folders
            .borrow_mut()
            .entry(folder.to_owned())
            .or_insert_with(|| self.link.list(folder))
            .clone()
    }

    fn read(&self, path: &str) -> Option<Arc<Vec<u8>>> {
        self.link.read(path).map(Arc::new)
    }
}

/// 標準入力 `input` から求めを読み、答えを `output` に書く（入力が尽きるまで）。
pub fn serve(input: impl BufRead + Send + 'static, output: impl Write + Send + 'static) {
    let (requests, asked) = mpsc::channel::<Value>();
    let (answers, answered) = mpsc::channel::<Value>();
    // ファイルの答えは、求めを描いている間に届くので、読み手を分けて振り分ける（拡張は求めをまとめて送ってよい）
    std::thread::spawn(move || {
        for line in input.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let value = serde_json::from_str::<Value>(&line)
                .unwrap_or_else(|e| json!({ "invalid": e.to_string() }));
            let sent = if value.get("answer").is_some() {
                answers.send(value)
            } else {
                requests.send(value)
            };
            if sent.is_err() {
                break;
            }
        }
    });
    let link = Link {
        out: Mutex::new(Box::new(output)),
        answers: Mutex::new(answered),
        next: AtomicU64::new(0),
    };
    let link = Arc::new(link);
    let mut images = HashMap::new();
    for request in asked {
        let reply = reply(request, &link, &mut images);
        link.send(&reply);
    }
}

/// 求め 1 つの答え（`id` は求めのものをそのまま返す）。
fn reply(request: Value, link: &Arc<Link>, images: &mut HashMap<String, ImageSource>) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let result = match request.get("invalid") {
        Some(Value::String(e)) => Err(e.clone()),
        _ => serde_json::from_value::<Request>(request)
            .map_err(|e| e.to_string())
            .and_then(|r| handle(r, link, images)),
    };
    let mut reply = match result {
        Ok(Value::Object(fields)) => {
            let mut reply = serde_json::Map::new();
            reply.insert("ok".into(), true.into());
            reply.extend(fields);
            Value::Object(reply)
        }
        Ok(_) => json!({ "ok": true }),
        Err(message) => json!({ "ok": false, "message": message }),
    };
    reply["id"] = id;
    reply
}

fn handle(
    request: Request,
    link: &Arc<Link>,
    images: &mut HashMap<String, ImageSource>,
) -> Result<Value, String> {
    match request {
        Request::Diagram {
            language,
            text,
            from,
            colors,
            scheme,
            imported_theme,
            time_zone,
            utc_offset,
        } => {
            // 画像を読む回数・バイト数の上限は、Markdown のファイルごと
            let images = from.map(|from| {
                images
                    .entry(from.clone())
                    .or_insert_with(|| {
                        let link = link.clone();
                        ImageSource {
                            from,
                            read: Arc::new(move |path: &str| {
                                link.read(path)
                                    .filter(|d| d.len() <= diagram::MAX_IMAGE_BYTES)
                            }),
                        }
                        .bounded()
                    })
                    .clone()
            });
            let zone = match (time_zone, utc_offset) {
                (Some(name), _) => {
                    Some(Arc::new(NamedZone(named_zone(&name))) as Arc<dyn diagram::TimeZone>)
                }
                (None, Some(minutes)) => Some(Arc::new(FixedZone(f64::from(minutes) * 60_000.0))
                    as Arc<dyn diagram::TimeZone>),
                (None, None) => None,
            };
            let scheme = Scheme::from_key(scheme.as_deref().unwrap_or(diagram::DEFAULT_KEY));
            // 読み込んだテーマが読めなければ、そのわけを添える（図は構文の色で描く。コアと同じ）
            let (imported, theme_error) = match (scheme, imported_theme) {
                (Scheme::Imported, Some(text)) => match MermaidConfig::parse(&text) {
                    Ok(config) => (Some(config), None),
                    Err(error) => (None, Some(error)),
                },
                _ => (None, None),
            };
            let paint = Paint {
                scheme,
                imported: imported.as_ref(),
                colors: &colors,
            };
            let mut reply = match diagram::draw_with(&language, &text, zone, images.as_ref()) {
                // 図の言葉でない・pele が読まない種類（拡張はコードとして見せる）
                None => json!({}),
                Some(Drawn::Svg(svg, config)) => paint
                    .colored(&svg, config.as_ref())
                    .map_or_else(|| json!({}), |svg| json!({ "svg": svg })),
                Some(Drawn::Noted(svg, note)) => paint
                    .colored(&svg, None)
                    .map_or_else(|| json!({}), |svg| json!({ "svg": svg, "note": note })),
                Some(Drawn::Error(error)) => json!({ "error": error }),
            };
            if let Some(error) = theme_error {
                reply["themeError"] = error.into();
            }
            Ok(reply)
        }
        Request::Math {
            formulas,
            color,
            time_limit,
        } => {
            // 手伝いはファイルごとに起動するので、そのファイルの上限としてコアに渡す（アプリの設定と同じ）
            if let Some(seconds) = time_limit {
                math::set_file_time_limit(
                    (seconds > 0).then(|| std::time::Duration::from_secs(seconds)),
                );
            }
            let jobs = formulas.into_iter().map(|f| (f.tex, f.display)).collect();
            let drawn: Vec<Value> = math::render_formulas(jobs)
                .into_iter()
                .map(|result| match result {
                    Ok(picture) => match math::colorize(&picture.svg, &color) {
                        Some(svg) => {
                            let mut value = serde_json::to_value(&picture).unwrap_or_default();
                            value["svg"] = svg.into();
                            value
                        }
                        // 色を当てられない式は、原文のまま
                        None => json!({}),
                    },
                    Err(error) => json!({ "error": error }),
                })
                .collect();
            Ok(json!({ "formulas": drawn }))
        }
        Request::VectorDrawable {
            path,
            data,
            fallback,
            dark,
        } => {
            let xml = decode_base64(&data).ok_or("The data isn’t base64")?;
            let files = LinkFiles {
                link,
                folders: RefCell::default(),
            };
            let picture = vectordrawable::picture(
                path.trim_matches('/'),
                &xml,
                &files,
                fallback.as_deref(),
                dark,
            )
            .map_err(|e| e.to_string())?;
            Ok(json!({ "svg": picture.svg, "notes": picture.notes }))
        }
        Request::SvgCheck { data } => {
            let data = decode_base64(&data).ok_or("The data isn’t base64")?;
            Ok(svg_check(&data))
        }
    }
}

/// 図に当てる配色（設定の配色と、読み込んだテーマの中身）と画面の色。
struct Paint<'a> {
    scheme: Scheme,
    imported: Option<&'a MermaidConfig>,
    colors: &'a Colors,
}

impl Paint<'_> {
    /// 図の SVG に配色を当てる（図の中で指定した Mermaid のテーマがあればそれが先。Mermaid のテーマは地も塗る）。
    fn colored(&self, svg: &str, own: Option<&MermaidConfig>) -> Option<String> {
        let theme = ThemeColors {
            text: self.colors.text.clone(),
            background: self.colors.background.clone(),
            kinds: self.colors.kinds.clone(),
        };
        let (vars, backdrop) =
            diagram::colors_for(own, self.scheme, self.imported, self.colors.dark, &theme);
        diagram::colorize(svg, &vars, backdrop)
    }
}

/// SVG のモバイル向けの確認（`issues`: targets・feature・message・count、`previews`: android・ios・flutter・
/// reactNative ごとの見え方 — `svg`（描ける SVG の文字列。部品が読めなければなし）・`title`・`caption`・
/// `messages`・`error`・`exact`）。
fn svg_check(data: &[u8]) -> Value {
    use svglint::Target;
    let issues: Vec<Value> = svglint::lint(data)
        .into_iter()
        .map(|i| {
            json!({
                "targets": i.targets.iter().map(|t| t.key()).collect::<Vec<_>>(),
                "feature": i.feature,
                "message": i.message,
                "count": i.count,
            })
        })
        .collect();
    let previews: serde_json::Map<String, Value> = [
        Target::Android,
        Target::Ios,
        Target::Flutter,
        Target::ReactNative,
    ]
    .into_iter()
    .map(|t| {
        let preview = svgmobile::preview(data, t);
        let mut value = json!({
            "title": preview.title,
            "caption": preview.caption,
            "messages": preview.messages,
            "exact": preview.exact,
        });
        if let Some(svg) = preview.svg {
            value["svg"] = String::from_utf8_lossy(&svg).into_owned().into();
        }
        if let Some(error) = preview.error {
            value["error"] = error.into();
        }
        (t.key().to_owned(), value)
    })
    .collect();
    json!({ "issues": issues, "previews": previews })
}

/// base64（標準の文字、`=` の詰め物はあってもなくても）を読む。読めなければ None。
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let text = text.trim_end_matches('=').as_bytes();
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
    for chunk in text.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Read};

    fn encode_base64(data: &[u8]) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(CHARS[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// 拡張の代わり: 求めを送り、ファイルの求めには `files` から答え（フォルダは中のファイルから作る）、答えを集める。
    fn exchange(requests: &[Value], files: &[(&str, &[u8])]) -> (Vec<Value>, Vec<Value>) {
        let (helper_in, mut to_helper) = std::io::pipe().unwrap();
        let (from_helper, helper_out) = std::io::pipe().unwrap();
        let server = std::thread::spawn(move || serve(BufReader::new(helper_in), helper_out));
        for request in requests {
            writeln!(to_helper, "{request}").unwrap();
        }
        let mut replies = Vec::new();
        let mut asks = Vec::new();
        let mut lines = BufReader::new(from_helper).lines();
        while replies.len() < requests.len() {
            let line = lines.next().unwrap().unwrap();
            let message: Value = serde_json::from_str(&line).unwrap();
            if let Some(n) = message.get("ask") {
                let answer = if let Some(path) = message["read"].as_str() {
                    match files.iter().find(|(p, _)| *p == path) {
                        Some((_, data)) => json!({ "answer": n, "data": encode_base64(data) }),
                        None => json!({ "answer": n, "data": null }),
                    }
                } else {
                    let folder = message["list"].as_str().unwrap();
                    let prefix = if folder.is_empty() {
                        String::new()
                    } else {
                        format!("{folder}/")
                    };
                    let mut entries: Vec<(String, bool)> = files
                        .iter()
                        .filter_map(|(p, _)| p.strip_prefix(&prefix))
                        .map(|rest| match rest.split_once('/') {
                            Some((dir, _)) => (dir.to_owned(), true),
                            None => (rest.to_owned(), false),
                        })
                        .collect();
                    entries.dedup();
                    let entries: Vec<Value> = entries
                        .into_iter()
                        .map(|(name, folder)| json!({ "name": name, "folder": folder }))
                        .collect();
                    json!({ "answer": n, "entries": entries })
                };
                writeln!(to_helper, "{answer}").unwrap();
                asks.push(message);
            } else {
                replies.push(message);
            }
        }
        drop(to_helper);
        server.join().unwrap();
        // 入力を閉じたら、それ以上は何も書かずに終わる
        let mut rest = String::new();
        for line in lines {
            rest.push_str(&line.unwrap());
        }
        assert!(rest.is_empty(), "{rest}");
        (replies, asks)
    }

    fn colors() -> Value {
        json!({ "dark": true, "text": "#abcdef", "background": "#101010", "kinds": { "keyword": "#ff0000" } })
    }

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 128, 7, 9]] {
            assert_eq!(decode_base64(&encode_base64(data)).unwrap(), data);
        }
        assert_eq!(decode_base64("YWI").unwrap(), b"ab");
        assert!(decode_base64("a").is_none());
        assert!(decode_base64("a*bc").is_none());
    }

    #[test]
    fn draws_a_diagram_in_the_given_colors() {
        let (replies, asks) = exchange(
            &[
                json!({ "id": 1, "op": "diagram", "language": "mermaid", "text": "flowchart LR\n  A --> B", "colors": colors(), "scheme": "syntax" }),
                json!({ "id": 2, "op": "diagram", "language": "mermaid", "text": "flowchart LR\n  A -->", "colors": colors() }),
                json!({ "id": 3, "op": "diagram", "language": "rust", "text": "fn main() {}", "colors": colors() }),
            ],
            &[],
        );
        assert!(asks.is_empty());
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["ok"], true);
        let svg = replies[0]["svg"].as_str().unwrap();
        assert!(svg.contains("<svg") && svg.contains("#abcdef"), "{svg}");
        assert!(!svg.contains("var(--pele"), "{svg}");
        assert!(replies[1]["error"].is_string(), "{}", replies[1]);
        assert_eq!(replies[2], json!({ "id": 3, "ok": true }));
    }

    #[test]
    fn a_dot_diagram_asks_for_its_images_from_the_markdown_folder() {
        let image = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>"#;
        let (replies, asks) = exchange(
            &[json!({
                "id": "a", "op": "diagram", "language": "dot", "from": "docs/a.md",
                "text": "digraph { a [image=\"img/a.svg\" shape=box] }", "colors": colors(),
            })],
            &[("docs/img/a.svg", image)],
        );
        assert_eq!(asks.len(), 1, "{asks:?}");
        assert_eq!(asks[0]["read"], "docs/img/a.svg");
        assert_eq!(replies[0]["id"], "a");
        let svg = replies[0]["svg"].as_str().unwrap();
        assert!(svg.contains("<image"), "{svg}");
    }

    #[test]
    fn draws_the_formulas_of_a_file_in_order() {
        let (replies, _) = exchange(
            &[json!({
                "id": 1, "op": "math", "color": "#abcdef",
                "formulas": [{ "tex": "x^2", "display": true }, { "tex": "\\frac{1" }, { "tex": "y" }],
            })],
            &[],
        );
        let formulas = replies[0]["formulas"].as_array().unwrap();
        assert_eq!(formulas.len(), 3);
        let svg = formulas[0]["svg"].as_str().unwrap();
        assert!(
            svg.contains("#abcdef") && !svg.contains("--math-fg"),
            "{svg}"
        );
        assert!(formulas[0]["width"].as_f64().unwrap() > 0.0);
        assert!(formulas[0]["depth"].is_number());
        assert!(formulas[1]["error"].is_string(), "{}", formulas[1]);
        assert!(formulas[2]["svg"].is_string());
    }

    #[test]
    fn a_vector_drawable_reads_its_colors_from_the_commit() {
        let xml = br#"<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="24dp" android:height="24dp" android:viewportWidth="24" android:viewportHeight="24">
<path android:fillColor="@color/brand" android:pathData="M0,0h24v24h-24z"/>
<path android:fillColor="?attr/colorControlNormal" android:pathData="M0,0h12v12h-12z"/>
</vector>"#;
        let (replies, asks) = exchange(
            &[json!({
                "id": 7, "op": "vectorDrawable", "path": "res/drawable/icon.xml",
                "data": encode_base64(xml), "fallback": "#abcdef",
            })],
            &[
                ("res/drawable/icon.xml", xml),
                (
                    "res/values/colors.xml",
                    br##"<resources><color name="brand">#FF112233</color></resources>"##,
                ),
            ],
        );
        assert!(
            asks.iter().any(|a| a["read"] == "res/values/colors.xml"),
            "{asks:?}"
        );
        let svg = replies[0]["svg"].as_str().unwrap();
        assert!(svg.contains("#112233"), "{svg}");
        assert!(svg.to_ascii_lowercase().contains("abcdef"), "{svg}");
        assert!(!replies[0]["notes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn checks_an_svg_for_mobile_renderers() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><style>circle{fill:red}</style><circle r="5"/></svg>"#;
        let (replies, _) = exchange(
            &[json!({ "id": 1, "op": "svgCheck", "data": encode_base64(svg) })],
            &[],
        );
        assert!(!replies[0]["issues"].as_array().unwrap().is_empty());
        for target in ["android", "ios", "flutter", "reactNative"] {
            let preview = &replies[0]["previews"][target];
            // 描ける SVG か、部品が読めなかったわけ
            assert!(
                preview["svg"].as_str().is_some_and(|s| s.contains("<svg"))
                    || preview["error"].is_string(),
                "{}",
                replies[0]
            );
            assert!(!preview["title"].as_str().unwrap().is_empty(), "{preview}");
            assert!(
                !preview["caption"].as_str().unwrap().is_empty(),
                "{preview}"
            );
            assert!(preview["messages"].is_array(), "{preview}");
        }
        // Android と flutter_svg は本物の取り込み、iOS と react-native-svg は近似（アプリと同じ）
        assert_eq!(replies[0]["previews"]["android"]["exact"], true);
        assert_eq!(replies[0]["previews"]["flutter"]["exact"], true);
        assert_eq!(replies[0]["previews"]["ios"]["exact"], false);
        assert_eq!(replies[0]["previews"]["reactNative"]["exact"], false);
    }

    #[test]
    fn a_mobile_renderer_that_cant_read_the_svg_gives_its_reason() {
        let (replies, _) = exchange(
            &[json!({ "id": 1, "op": "svgCheck", "data": encode_base64(b"<svg") })],
            &[],
        );
        let android = &replies[0]["previews"]["android"];
        assert!(android["error"].is_string(), "{android}");
        assert!(android.get("svg").is_none(), "{android}");
    }

    #[test]
    fn a_diagram_takes_the_colors_of_the_chosen_scheme() {
        let flow = "flowchart LR\n  A --> B";
        let draw = |fields: Value| {
            let mut request = json!({ "id": 1, "op": "diagram", "language": "mermaid", "text": flow, "colors": colors() });
            for (k, v) in fields.as_object().unwrap() {
                request[k] = v.clone();
            }
            let (replies, _) = exchange(&[request], &[]);
            replies.into_iter().next().unwrap()
        };
        // 構文の色: 画面の文字の色
        let syntax = draw(json!({ "scheme": "syntax" }));
        assert!(
            syntax["svg"].as_str().unwrap().contains("#abcdef"),
            "{syntax}"
        );
        // 既定はアプリと同じ、外観に合わせた Mermaid のテーマ（画面の色は使わない）
        let mermaid = draw(json!({}));
        let svg = mermaid["svg"].as_str().unwrap();
        assert!(!svg.contains("#abcdef"), "{svg}");
        assert_eq!(draw(json!({ "scheme": "mermaid" }))["svg"], mermaid["svg"]);
        // 名前つきのテーマは、外観に関係なくそのテーマ
        let forest = draw(json!({ "scheme": "mermaid-forest" }));
        assert_ne!(forest["svg"], mermaid["svg"]);
        // 読み込んだテーマ
        let imported = draw(json!({
            "scheme": "mermaid-import:mine",
            "importedTheme": r##"{"theme": "base", "themeVariables": {"primaryColor": "#123456"}}"##,
        }));
        assert!(
            imported["svg"].as_str().unwrap().contains("#123456"),
            "{imported}"
        );
        assert!(imported.get("themeError").is_none(), "{imported}");
        // 読めないテーマは、そのわけを添えて構文の色で
        let broken = draw(json!({ "scheme": "mermaid-import:mine", "importedTheme": "not json" }));
        assert!(broken["themeError"].is_string(), "{broken}");
        assert!(
            broken["svg"].as_str().unwrap().contains("#abcdef"),
            "{broken}"
        );
    }

    #[test]
    fn a_named_time_zone_follows_daylight_saving() {
        use diagram::TimeZone as _;
        let zone = NamedZone(named_zone("America/New_York"));
        // 2024-01-15 と 2024-07-15 の UTC の正午
        assert_eq!(zone.offset_ms(1_705_320_000_000.0), -5.0 * 3_600_000.0);
        assert_eq!(zone.offset_ms(1_721_044_800_000.0), -4.0 * 3_600_000.0);
        assert_eq!(
            NamedZone(named_zone("UTC")).offset_ms(1_721_044_800_000.0),
            0.0
        );
        assert_eq!(zone.offset_ms(f64::NAN), 0.0);
        // 読めない名前は OS の時間帯（アプリと同じ）
        let system = NamedZone(jiff::tz::TimeZone::system());
        assert_eq!(
            NamedZone(named_zone("Nowhere/Nothing")).offset_ms(1_721_044_800_000.0),
            system.offset_ms(1_721_044_800_000.0)
        );
    }

    #[test]
    fn a_gantt_counts_its_dates_in_the_given_time_zone() {
        let gantt = "gantt\n  dateFormat X\n  axisFormat %H:%M\n  section S\n  T : 0, 3600";
        let draw = |zone: Value| {
            let mut request = json!({ "id": 1, "op": "diagram", "language": "mermaid", "text": gantt, "colors": colors() });
            if !zone.is_null() {
                request["timeZone"] = zone;
            }
            let (replies, _) = exchange(&[request], &[]);
            replies.into_iter().next().unwrap()["svg"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        };
        let utc = draw(json!("UTC"));
        assert!(utc.contains("00:00"), "{utc}");
        assert_eq!(draw(Value::Null), utc);
        let tokyo = draw(json!("Asia/Tokyo"));
        assert!(tokyo.contains("09:00"), "{tokyo}");
    }

    #[test]
    fn the_math_time_limit_is_taken_from_the_request() {
        let (replies, _) = exchange(
            &[json!({
                "id": 1, "op": "math", "color": "#abcdef", "timeLimit": 0,
                "formulas": [{ "tex": "x^2" }],
            })],
            &[],
        );
        assert!(
            replies[0]["formulas"][0]["svg"].is_string(),
            "{}",
            replies[0]
        );
        // 上限は 1 つのファイルの間だけでなく、手伝いの起動ごと（拡張はファイルごとに起動する）
        math::set_file_time_limit(Some(std::time::Duration::from_secs(2)));
    }

    #[test]
    fn a_bad_request_gets_an_error_and_the_next_one_is_answered() {
        let (helper_in, mut to_helper) = std::io::pipe().unwrap();
        let (mut from_helper, helper_out) = std::io::pipe().unwrap();
        let server = std::thread::spawn(move || serve(BufReader::new(helper_in), helper_out));
        writeln!(to_helper, "not json").unwrap();
        writeln!(to_helper, r#"{{"id":2,"op":"unknown"}}"#).unwrap();
        writeln!(to_helper, r#"{{"id":3,"op":"svgCheck","data":"*"}}"#).unwrap();
        drop(to_helper);
        server.join().unwrap();
        let mut out = String::new();
        from_helper.read_to_string(&mut out).unwrap();
        let replies: Vec<Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 3, "{out}");
        assert_eq!(replies[0]["ok"], false);
        assert_eq!(replies[0]["id"], Value::Null);
        assert_eq!(replies[1]["ok"], false);
        assert_eq!(replies[1]["id"], 2);
        assert_eq!(replies[2]["ok"], false);
        assert!(replies[2]["message"].is_string());
    }
}
