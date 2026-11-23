//! treeView のアイコン（pele の `src/diagrams/treeview/icons.ts`）。

use super::model::NodeType;
use crate::preprocess::Config;
use crate::util::yaml::{Map, Value};

/// Mermaid の組み込みのパックの名前。組み込みは `file` と `folder` だけで、ほかの名前はホストのアイコンに渡す。
pub const BUILTIN_PACK: &str = "mermaid-treeview";

/// 24 × 24 の枠の輪郭。
pub fn glyph(name: &str) -> Option<&'static str> {
    match name {
        "folder" => Some(
            "M3 6.5A1.5 1.5 0 0 1 4.5 5h4.6l2.4 2.5h8A1.5 1.5 0 0 1 21 9v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z",
        ),
        "file" => Some(
            "M6.5 3h7L19 8.5v11a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 5 19.5v-15A1.5 1.5 0 0 1 6.5 3zM13.5 3v5.5H19",
        ),
        _ => None,
    }
}

pub struct IconConfig<'a> {
    pub show_icons: bool,
    pub default_icon_pack: String,
    pub filename_icons: Option<&'a Map>,
    pub extension_icons: Option<&'a Map>,
}

fn own<'a>(map: Option<&'a Map>, key: &str) -> Option<&'a str> {
    map?.get(key)?.as_str()
}

/// 設定の表からファイルのアイコン。ファイル名が拡張子に勝ち、拡張子は小文字で、頭の点があってもなくても合う。
pub fn detect_icon<'a>(name: &str, config: &IconConfig<'a>) -> Option<&'a str> {
    if let Some(by_name) = own(config.filename_icons, name).filter(|s| !s.is_empty()) {
        return Some(by_name);
    }
    let dot = name.rfind('.')?;
    if dot > 0 {
        let ext = name[dot..].to_lowercase();
        return own(config.extension_icons, &ext)
            .or_else(|| own(config.extension_icons, &ext[1..]));
    }
    None
}

fn qualify_icon(icon: &str, default_icon_pack: &str) -> String {
    if icon.contains(':') {
        icon.to_string()
    } else if glyph(icon).is_some() || default_icon_pack.is_empty() {
        format!("{BUILTIN_PACK}:{icon}")
    } else {
        format!("{default_icon_pack}:{icon}")
    }
}

/// ノードのアイコンの名前（パックつき）。なければ None。`icon()` の注釈がいつも勝ち、ほかは `showIcons` のときだけ。
pub fn get_node_icon(
    icon: Option<&str>,
    name: &str,
    node_type: NodeType,
    config: &IconConfig,
) -> Option<String> {
    if icon == Some("none") {
        return None;
    }
    if let Some(icon) = icon.filter(|i| !i.is_empty()) {
        return Some(qualify_icon(icon, &config.default_icon_pack));
    }
    if !config.show_icons {
        return None;
    }
    if node_type == NodeType::File {
        let detected = detect_icon(name, config);
        if detected == Some("none") {
            return None;
        }
        if let Some(d) = detected.filter(|d| !d.is_empty()) {
            return Some(qualify_icon(d, &config.default_icon_pack));
        }
    }
    Some(format!(
        "{BUILTIN_PACK}:{}",
        if node_type == NodeType::Directory {
            "folder"
        } else {
            "file"
        }
    ))
}

pub fn icon_config(config: &Config) -> IconConfig<'_> {
    fn map(v: Option<&Value>) -> Option<&Map> {
        v.and_then(Value::as_obj)
    }
    let c = map(config.get("treeView"));
    let get = |key: &str| c.and_then(|c| c.get(key));
    IconConfig {
        show_icons: get("showIcons").is_some_and(Value::truthy),
        default_icon_pack: get("defaultIconPack")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        filename_icons: map(get("filenameIcons")),
        extension_icons: map(get("extensionIcons")),
    }
}
