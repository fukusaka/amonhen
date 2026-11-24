//! History の「すべてのファイル」で、Android の VectorDrawable（拡張子が `.xml` で、根が `<vector>`）を絵で見せる。
//!
//! 絵は `svgandroid` で SVG に戻して描く。色の参照（`@color/x`）は、同じコミットの、drawable のいちばん近い `res`
//! フォルダの `values*/` の `<color>`（と `color/x.xml` の色の一覧）から引く。テーマの属性（`?attr/x`）と、
//! 見つからない参照は引けないので、FE が渡す画面の文字の色（なければ灰色）で描き、そのことを説明に足す。`<animated-vector>` は、動かす前の絵（中に
//! 書いた `<vector>` か、同じ `res` の `@drawable/x`）を描く。読むのはコミットの中のものだけで、量に上限がある。

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use amonhen_model::{Result, tr};

use crate::svgandroid::{self, XmlNode};

/// 根の要素を探すのに読む、ファイルの頭の長さ（ふつうの XML のファイルはこれだけで見分ける）。
pub const SNIFF_BYTES: usize = 4096;
/// 絵にする VectorDrawable の大きさの上限（超えたらテキストで見せる）。
pub const MAX_DRAWABLE_BYTES: usize = 4 << 20;
/// 色を探して読む資源のファイルの、1 つの大きさの上限（超えたものは読まない）。
pub const MAX_RESOURCE_FILE_BYTES: usize = 1 << 20;
/// 色を探して読む資源のファイルの数の上限。
pub const MAX_RESOURCE_FILES: usize = 64;
/// 色を探して読む資源のファイルの、合わせた大きさの上限。
pub const MAX_RESOURCE_BYTES: usize = 8 << 20;
/// 色の参照をたどる深さの上限（`@color/a` → `@color/b` → …）。
pub const MAX_REFERENCE_DEPTH: usize = 8;
/// 引けない色の代わりに描く色の既定（FE が文字の色を渡さないとき。明るい地でも暗い地でも見える灰色）。
pub const FALLBACK_COLOR: &str = "#FF808080";

/// コミットの中のファイルを読むもの（テストでは手で作る）。パスは根からの相対で、`/` で区切る。
pub trait Files {
    /// フォルダ `folder`（空なら根）の中のもの（名前と、フォルダか）。読めなければ空。
    fn list(&self, folder: &str) -> Vec<(String, bool)>;
    /// ファイル `path` の中身。なければ None。
    fn read(&self, path: &str) -> Option<Arc<Vec<u8>>>;
}

/// 絵にした VectorDrawable。
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    /// 描ける SVG（大きさは dp）。
    pub svg: String,
    /// 絵の下に添える説明（引けなかった色・SVG では同じに描けないもの。訳した文）。
    pub notes: Vec<String>,
}

/// 拡張子が `.xml` か。
pub fn is_xml(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("xml"))
}

/// XML の根の要素の名前（接頭辞を除く）。頭の `SNIFF_BYTES` だけを見る。宣言・コメント・DOCTYPE は飛ばす。
pub fn root_element(data: &[u8]) -> Option<&str> {
    let head = &data[..data.len().min(SNIFF_BYTES)];
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let mut i = 0;
    loop {
        while i < head.len() && head[i].is_ascii_whitespace() {
            i += 1;
        }
        let rest = &head[i..];
        let skip = |end: &[u8]| {
            rest.windows(end.len())
                .position(|w| w == end)
                .map(|p| p + end.len())
        };
        if rest.starts_with(b"<?") {
            i += skip(b"?>")?;
        } else if rest.starts_with(b"<!--") {
            i += skip(b"-->")?;
        } else if rest.starts_with(b"<!") {
            i += skip(b">")?;
        } else if rest.starts_with(b"<") {
            let name = &rest[1..];
            let end = name
                .iter()
                .position(|&b| b.is_ascii_whitespace() || b == b'>' || b == b'/')?;
            let name = std::str::from_utf8(&name[..end]).ok()?;
            return Some(name.rsplit(':').next().unwrap_or(name)).filter(|n| !n.is_empty());
        } else {
            return None;
        }
    }
}

/// History の「すべてのファイル」で絵にする VectorDrawable か（`.xml` で、根が `<vector>` か `<animated-vector>`）。
/// 頭だけを見るので、ふつうの XML のファイルにかかる手間は小さい。
pub fn is_vector_drawable(path: &str, data: &[u8]) -> bool {
    is_xml(path) && matches!(root_element(data), Some("vector" | "animated-vector"))
}

/// コミットの `path` の VectorDrawable（中身 `xml`）を絵にする。色の参照・`<animated-vector>` の参照は `files`
/// から引く。引けない色は `fallback`（画面の文字の色の `#rrggbb`。テーマの属性はたいてい文字・アイコンの色）、
/// なければ `FALLBACK_COLOR` で描く。
pub fn picture(
    path: &str,
    xml: &[u8],
    files: &dyn Files,
    fallback: Option<&str>,
) -> Result<Picture> {
    if xml.len() > MAX_DRAWABLE_BYTES {
        return Err(tr!("The VectorDrawable is too large to draw").into());
    }
    let res = res_folder(path);
    let mut text = String::from_utf8_lossy(xml).into_owned();
    if root_element(xml) == Some("animated-vector") {
        text = animated_drawable(text, res.as_deref(), files)?;
    }
    let colors = Colors {
        files,
        res: res.as_deref(),
        table: OnceCell::new(),
        themed: RefCell::new(Vec::new()),
        missing: RefCell::new(Vec::new()),
        fallback: fallback
            .filter(|c| svgandroid::is_color(c))
            .unwrap_or(FALLBACK_COLOR),
    };
    let resolve = |value: &str| colors.resolve(value);
    let (svg, converter_notes) = svgandroid::convert(&text, Some(&resolve))?;
    let mut notes = Vec::new();
    let themed = colors.themed.into_inner();
    if !themed.is_empty() {
        notes.push(tr!(
            "Android theme attributes can’t be resolved here, so they’re drawn in the text color: {refs}",
            refs = themed.join(", ")
        ));
    }
    let missing = colors.missing.into_inner();
    if !missing.is_empty() {
        notes.push(tr!(
            "Colors not found in this commit’s resources are drawn in the text color: {refs}",
            refs = missing.join(", ")
        ));
    }
    notes.extend(converter_notes);
    Ok(Picture { svg, notes })
}

/// `<animated-vector>` の、動かす前の絵の XML。中に書いた `<vector>` ならそのまま（`svgandroid` が中を見る）、
/// `@drawable/x` なら同じ `res` の `drawable*/x.xml`。
fn animated_drawable(text: String, res: Option<&str>, files: &dyn Files) -> Result<String> {
    let nodes = svgandroid::parse_xml(&text);
    let Some(animated) = nodes.iter().find(|n| n.local() == "animated-vector") else {
        return Ok(text);
    };
    if svgandroid::inline_drawable(animated).is_some() {
        return Ok(text);
    }
    let Some(reference) = animated.attr("drawable") else {
        return Err(tr!("The animated vector has no drawable").into());
    };
    let not_found = || {
        tr!(
            "The animated vector’s drawable isn’t in this commit: {value}",
            value = reference.trim()
        )
    };
    let name = parse_reference(&reference)
        .filter(|r| r.kind == "drawable" && !r.framework)
        .map(|r| r.name)
        .ok_or_else(not_found)?;
    let res = res.ok_or_else(not_found)?;
    let file = format!("{name}.xml");
    for folder in qualified_folders(files, res, "drawable") {
        let has = files
            .list(&join(res, &folder))
            .iter()
            .any(|(n, dir)| !dir && *n == file);
        if !has {
            continue;
        }
        let data = files
            .read(&join(&join(res, &folder), &file))
            .ok_or_else(not_found)?;
        if data.len() > MAX_DRAWABLE_BYTES {
            return Err(tr!("The VectorDrawable is too large to draw").into());
        }
        if root_element(&data) != Some("vector") {
            return Err(tr!(
                "The animated vector’s drawable isn’t a <vector>: {value}",
                value = reference.trim()
            )
            .into());
        }
        return Ok(String::from_utf8_lossy(&data).into_owned());
    }
    Err(not_found().into())
}

/// drawable のいちばん近い `res` フォルダ。drawable が `drawable*/`・`mipmap*/` にあればその親、なければ上の
/// フォルダの `res`。見つからなければ None（色の参照は引けない）。
fn res_folder(path: &str) -> Option<String> {
    let folders: Vec<&str> = path.split('/').collect();
    let folders = &folders[..folders.len().saturating_sub(1)];
    if let Some((last, parent)) = folders.split_last()
        && (last.starts_with("drawable") || last.starts_with("mipmap"))
    {
        return Some(parent.join("/"));
    }
    let at = folders.iter().rposition(|f| *f == "res")?;
    Some(folders[..=at].join("/"))
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// `res` の中の `kind` と `kind-*` のフォルダ（修飾のないものを先に、あとは名前の順）。
fn qualified_folders(files: &dyn Files, res: &str, kind: &str) -> Vec<String> {
    let prefix = format!("{kind}-");
    let mut folders: Vec<String> = files
        .list(res)
        .into_iter()
        .filter(|(name, dir)| *dir && (name == kind || name.starts_with(&prefix)))
        .map(|(name, _)| name)
        .collect();
    folders.sort_by(|a, b| (a != kind, a).cmp(&(b != kind, b)));
    folders
}

/// 資源の参照（`@color/x`・`@android:color/x`・`@drawable/x`）。
struct Reference {
    framework: bool,
    kind: String,
    name: String,
}

fn parse_reference(value: &str) -> Option<Reference> {
    let rest = value.trim().strip_prefix('@')?;
    let rest = rest.strip_prefix('+').unwrap_or(rest);
    let rest = rest.strip_prefix('*').unwrap_or(rest);
    let (framework, rest) = match rest.split_once(':') {
        Some((package, rest)) => (package == "android", rest),
        None => (false, rest),
    };
    let (kind, name) = rest.split_once('/')?;
    Some(Reference {
        framework,
        kind: kind.to_owned(),
        name: name.to_owned(),
    })
}

/// Android が持つ色のうち、よく使うもの（`@android:color/x`）。
fn framework_color(name: &str) -> Option<&'static str> {
    Some(match name {
        "white" => "#FFFFFFFF",
        "black" => "#FF000000",
        "transparent" => "#00000000",
        "darker_gray" => "#FFAAAAAA",
        "background_dark" => "#FF000000",
        "background_light" => "#FFFFFFFF",
        _ => return None,
    })
}

/// 色の参照を引くもの。引けなかったものは覚えておき、説明に足す。
struct Colors<'a> {
    files: &'a dyn Files,
    res: Option<&'a str>,
    /// `values*/` の `<color>`（名前 → 値）。最初に引くときに読む。
    table: OnceCell<HashMap<String, String>>,
    themed: RefCell<Vec<String>>,
    missing: RefCell<Vec<String>>,
    /// 引けない色の代わりに描く色。
    fallback: &'a str,
}

impl Colors<'_> {
    fn resolve(&self, value: &str) -> String {
        let original = value.trim();
        let mut current = original.to_owned();
        for _ in 0..=MAX_REFERENCE_DEPTH {
            if svgandroid::is_color(&current) {
                return current;
            }
            if current.starts_with('?') {
                return self.fallback(&self.themed, original);
            }
            if current == "@null" {
                return "#00000000".into();
            }
            let Some(next) = parse_reference(&current).and_then(|r| self.lookup(&r)) else {
                break;
            };
            current = next;
        }
        self.fallback(&self.missing, original)
    }

    fn fallback(&self, list: &RefCell<Vec<String>>, value: &str) -> String {
        let mut list = list.borrow_mut();
        if !list.iter().any(|v| v == value) {
            list.push(value.to_owned());
        }
        self.fallback.into()
    }

    fn lookup(&self, r: &Reference) -> Option<String> {
        if r.kind != "color" {
            return None;
        }
        if r.framework {
            return framework_color(&r.name).map(str::to_owned);
        }
        if let Some(value) = self.table().get(&r.name) {
            return Some(value.clone());
        }
        self.state_list(&r.name)
    }

    fn table(&self) -> &HashMap<String, String> {
        self.table.get_or_init(|| match self.res {
            Some(res) => read_color_table(self.files, res),
            None => HashMap::new(),
        })
    }

    /// `color*/name.xml`（色の一覧 `<selector>`）の、状態のないときの色（状態を書かない `<item>`、なければ最後）。
    fn state_list(&self, name: &str) -> Option<String> {
        let res = self.res?;
        let file = format!("{name}.xml");
        let folder = qualified_folders(self.files, res, "color")
            .into_iter()
            .find(|f| {
                self.files
                    .list(&join(res, f))
                    .iter()
                    .any(|(n, dir)| !dir && *n == file)
            })?;
        let data = self.files.read(&join(&join(res, &folder), &file))?;
        if data.len() > MAX_RESOURCE_FILE_BYTES {
            return None;
        }
        let nodes = svgandroid::parse_xml(&String::from_utf8_lossy(&data));
        let selector = nodes.iter().find(|n| n.local() == "selector")?;
        let items: Vec<&XmlNode> = selector
            .children
            .iter()
            .filter(|c| c.local() == "item")
            .collect();
        let item = items
            .iter()
            .find(|item| !item.has_attr_prefix("state_"))
            .or(items.last())?;
        item.attr("color")
    }
}

/// `res` の `values` と `values-*` の XML の `<color>`（`<item type="color">` も）。修飾のない `values` を先に読み、
/// 同じ名前はそれを使う。読むのは上限まで。
fn read_color_table(files: &dyn Files, res: &str) -> HashMap<String, String> {
    let mut table = HashMap::new();
    let (mut count, mut bytes) = (0, 0);
    for folder in qualified_folders(files, res, "values") {
        let folder = join(res, &folder);
        let mut names: Vec<String> = files
            .list(&folder)
            .into_iter()
            .filter(|(name, dir)| !dir && is_xml(name))
            .map(|(name, _)| name)
            .collect();
        names.sort();
        for name in names {
            if count >= MAX_RESOURCE_FILES || bytes >= MAX_RESOURCE_BYTES {
                return table;
            }
            count += 1;
            let Some(data) = files.read(&join(&folder, &name)) else {
                continue;
            };
            if data.len() > MAX_RESOURCE_FILE_BYTES {
                continue;
            }
            bytes += data.len();
            let text = String::from_utf8_lossy(&data);
            if !text.contains("<color") && !text.contains("\"color\"") {
                continue;
            }
            for resources in svgandroid::parse_xml(&text)
                .iter()
                .filter(|n| n.local() == "resources")
            {
                for child in &resources.children {
                    let is_color = child.local() == "color"
                        || (child.local() == "item"
                            && child.attr("type").as_deref() == Some("color"));
                    if let (true, Some(name)) = (is_color, child.attr("name")) {
                        table.entry(name).or_insert_with(|| child.text.clone());
                    }
                }
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手で作るコミットの中身（パス → 中身）。
    #[derive(Default)]
    struct Tree {
        files: HashMap<String, Arc<Vec<u8>>>,
        reads: RefCell<Vec<String>>,
    }

    impl Tree {
        fn with(files: &[(&str, &str)]) -> Self {
            Tree {
                files: files
                    .iter()
                    .map(|(p, c)| ((*p).to_owned(), Arc::new(c.as_bytes().to_vec())))
                    .collect(),
                reads: RefCell::default(),
            }
        }
    }

    impl Files for Tree {
        fn list(&self, folder: &str) -> Vec<(String, bool)> {
            let prefix = if folder.is_empty() {
                String::new()
            } else {
                format!("{folder}/")
            };
            let mut out: Vec<(String, bool)> = Vec::new();
            for path in self.files.keys() {
                let Some(rest) = path.strip_prefix(&prefix) else {
                    continue;
                };
                let entry = match rest.split_once('/') {
                    Some((dir, _)) => (dir.to_owned(), true),
                    None => (rest.to_owned(), false),
                };
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            out
        }

        fn read(&self, path: &str) -> Option<Arc<Vec<u8>>> {
            self.reads.borrow_mut().push(path.to_owned());
            self.files.get(path).cloned()
        }
    }

    const NS: &str = r#"xmlns:android="http://schemas.android.com/apk/res/android""#;

    fn vector(body: &str) -> String {
        format!(
            r#"<vector {NS} android:width="24dp" android:height="24dp" android:viewportWidth="24" android:viewportHeight="24">{body}</vector>"#
        )
    }

    fn path(fill: &str) -> String {
        format!(r#"<path android:pathData="M0,0h24v24z" android:fillColor="{fill}"/>"#)
    }

    #[test]
    fn finds_the_root_element_past_the_prolog() {
        assert_eq!(
            root_element(
                b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<!-- c -->\n<!DOCTYPE x>\n<vector a=\"1\">"
            ),
            Some("vector")
        );
        assert_eq!(
            root_element(b"<aapt:animated-vector>"),
            Some("animated-vector")
        );
        assert_eq!(root_element(b"  <selector/>"), Some("selector"));
        assert_eq!(root_element(b"text"), None);
        assert_eq!(root_element(b"<!-- never closed"), None);
        // 頭の外にある根は見ない
        let mut long = b"<!--".to_vec();
        long.extend(std::iter::repeat_n(b' ', SNIFF_BYTES));
        long.extend(b"--><vector>");
        assert_eq!(root_element(&long), None);
    }

    #[test]
    fn only_xml_files_with_a_vector_root_are_drawables() {
        let v = vector("").into_bytes();
        assert!(is_vector_drawable("app/src/main/res/drawable/ic.xml", &v));
        assert!(is_vector_drawable("anywhere/IC.XML", &v));
        assert!(!is_vector_drawable("res/drawable/ic.svg", &v));
        assert!(is_vector_drawable(
            "res/drawable/a.xml",
            b"<animated-vector>"
        ));
        for other in ["<selector>", "<shape>", "<layer-list>", "<resources>"] {
            assert!(!is_vector_drawable("res/drawable/a.xml", other.as_bytes()));
        }
    }

    #[test]
    fn finds_the_nearest_res_folder() {
        assert_eq!(
            res_folder("app/src/main/res/drawable-v24/ic.xml").as_deref(),
            Some("app/src/main/res")
        );
        assert_eq!(res_folder("drawable/ic.xml").as_deref(), Some(""));
        assert_eq!(
            res_folder("lib/res/raw/icons/ic.xml").as_deref(),
            Some("lib/res")
        );
        assert_eq!(res_folder("docs/ic.xml"), None);
    }

    #[test]
    fn resolves_colors_from_values_following_chains() {
        let tree = Tree::with(&[
            (
                "app/res/values/colors.xml",
                r##"<resources><color name="brand">@color/primary</color><color name="primary">#ff0000</color>
                   <item name="accent" type="color">@android:color/white</item><color name="loop">@color/loop</color></resources>"##,
            ),
            (
                "app/res/values-night/colors.xml",
                r##"<resources><color name="primary">#00ff00</color><color name="night_only">#0000ff</color></resources>"##,
            ),
            ("app/res/values/strings.xml", "<resources/>"),
        ]);
        let xml = vector(
            &[
                path("@color/brand"),
                path("@color/accent"),
                path("@color/night_only"),
                path("@android:color/black"),
            ]
            .concat(),
        );
        let p = picture("app/res/drawable/ic.xml", xml.as_bytes(), &tree, None).unwrap();
        // values が values-night より先
        assert!(p.svg.contains(r##"fill="#ff0000""##), "{}", p.svg);
        assert!(p.svg.contains(r##"fill="#ffffff""##), "{}", p.svg);
        assert!(p.svg.contains(r##"fill="#0000ff""##), "{}", p.svg);
        assert!(p.svg.contains(r##"fill="#000000""##), "{}", p.svg);
        assert!(p.notes.is_empty(), "{:?}", p.notes);

        // 終わらない参照は深さで止め、代わりの色で描く
        let xml = vector(&path("@color/loop"));
        let p = picture("app/res/drawable/ic.xml", xml.as_bytes(), &tree, None).unwrap();
        assert!(p.svg.contains(r##"fill="#808080""##), "{}", p.svg);
        assert_eq!(
            p.notes,
            vec![tr!(
                "Colors not found in this commit’s resources are drawn in the text color: {refs}",
                refs = "@color/loop"
            )]
        );
    }

    #[test]
    fn draws_what_cant_be_resolved_in_the_text_color_with_a_note() {
        let tree = Tree::with(&[]);
        let xml = vector(
            &[
                path("?attr/colorControlNormal"),
                path("?android:attr/textColorPrimary"),
                path("?attr/colorControlNormal"),
                path("@color/nowhere"),
                path("@android:color/holo_blue_light"),
            ]
            .concat(),
        );
        let p = picture(
            "res/drawable/ic.xml",
            xml.as_bytes(),
            &tree,
            Some("#e5e5e5"),
        )
        .unwrap();
        assert_eq!(p.svg.matches(r##"fill="#e5e5e5""##).count(), 5, "{}", p.svg);
        // 文字の色がない（読めない）ときは灰色
        for fallback in [None, Some("white")] {
            let gray = picture("res/drawable/ic.xml", xml.as_bytes(), &tree, fallback).unwrap();
            assert_eq!(gray.svg.matches(r##"fill="#808080""##).count(), 5);
        }
        assert_eq!(
            p.notes,
            vec![
                tr!(
                    "Android theme attributes can’t be resolved here, so they’re drawn in the text color: {refs}",
                    refs = "?attr/colorControlNormal, ?android:attr/textColorPrimary"
                ),
                tr!(
                    "Colors not found in this commit’s resources are drawn in the text color: {refs}",
                    refs = "@color/nowhere, @android:color/holo_blue_light"
                ),
            ]
        );
    }

    #[test]
    fn resolves_tint_and_color_state_lists() {
        let tree = Tree::with(&[
            (
                "res/values/colors.xml",
                r##"<resources><color name="tint">@color/states</color></resources>"##,
            ),
            (
                "res/color/states.xml",
                r##"<selector><item android:state_enabled="false" android:color="#111111"/><item android:color="#00ff00"/></selector>"##,
            ),
        ]);
        let xml =
            vector(&path("#000")).replace("<vector ", r#"<vector android:tint="@color/tint" "#);
        let p = picture("res/drawable/ic.xml", xml.as_bytes(), &tree, None).unwrap();
        assert!(p.svg.contains(r##"fill="#00ff00""##), "{}", p.svg);
        assert!(p.notes.is_empty(), "{:?}", p.notes);
    }

    #[test]
    fn draws_an_animated_vector_inline_or_by_reference() {
        let inline = format!(
            r#"<animated-vector {NS} xmlns:aapt="http://schemas.android.com/aapt"><aapt:attr name="android:drawable">{}</aapt:attr><target android:name="x"/></animated-vector>"#,
            vector(&path("#f00"))
        );
        let tree = Tree::with(&[("res/drawable-v24/ic_base.xml", &vector(&path("#00f")))]);
        let p = picture("res/drawable/anim.xml", inline.as_bytes(), &tree, None).unwrap();
        assert!(p.svg.contains(r##"fill="#ff0000""##), "{}", p.svg);

        let by_ref = format!(
            r#"<animated-vector {NS} android:drawable="@drawable/ic_base"></animated-vector>"#
        );
        let p = picture("res/drawable/anim.xml", by_ref.as_bytes(), &tree, None).unwrap();
        assert!(p.svg.contains(r##"fill="#0000ff""##), "{}", p.svg);

        let missing = by_ref.replace("ic_base", "ic_gone");
        let error = picture("res/drawable/anim.xml", missing.as_bytes(), &tree, None).unwrap_err();
        assert_eq!(
            error.to_string(),
            tr!(
                "The animated vector’s drawable isn’t in this commit: {value}",
                value = "@drawable/ic_gone"
            )
        );
        let tree = Tree::with(&[("res/drawable/ic_base.xml", "<selector/>")]);
        assert!(picture("res/drawable/anim.xml", by_ref.as_bytes(), &tree, None).is_err());
    }

    #[test]
    fn caps_what_it_reads() {
        assert!(
            picture(
                "res/drawable/ic.xml",
                &vec![b' '; MAX_DRAWABLE_BYTES + 1],
                &Tree::default(),
                None
            )
            .is_err()
        );
        // 大きすぎる資源は読まず、数の上限を超えた分も読まない
        let big = format!(
            r##"<resources><color name="big">#ff0000</color>{}</resources>"##,
            " ".repeat(MAX_RESOURCE_FILE_BYTES)
        );
        let mut files = vec![("res/values/a_big.xml".to_owned(), big)];
        for i in 0..MAX_RESOURCE_FILES + 10 {
            files.push((
                format!("res/values/c{i:03}.xml"),
                format!(r##"<resources><color name="c{i}">#00ff00</color></resources>"##),
            ));
        }
        let pairs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let tree = Tree::with(&pairs);
        let last = MAX_RESOURCE_FILES + 5;
        let xml = vector(
            &[
                path("@color/big"),
                path("@color/c0"),
                path(&format!("@color/c{last}")),
            ]
            .concat(),
        );
        let p = picture("res/drawable/ic.xml", xml.as_bytes(), &tree, None).unwrap();
        assert!(p.svg.contains(r##"fill="#00ff00""##), "{}", p.svg);
        assert_eq!(
            p.notes,
            vec![tr!(
                "Colors not found in this commit’s resources are drawn in the text color: {refs}",
                refs = format!("@color/big, @color/c{last}")
            )]
        );
        assert!(tree.reads.borrow().len() <= MAX_RESOURCE_FILES + 1);
    }

    #[test]
    fn reads_no_resources_without_references() {
        let tree = Tree::with(&[("res/values/colors.xml", "<resources/>")]);
        let xml = vector(&path("#fff"));
        picture("res/drawable/ic.xml", xml.as_bytes(), &tree, None).unwrap();
        assert!(tree.reads.borrow().is_empty());
    }
}
