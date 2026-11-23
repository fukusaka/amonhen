//! C4 の形（pele の `src/diagrams/c4/types.ts`）。TS の形と境界（`C4Shape`・`C4Boundary`）は 1 つの
//! `C4Element` にまとめた。どちらにしかない項目は、もう一方では使わない。

/// `$key="value"` の引数。
#[derive(Clone, Debug, PartialEq)]
pub struct C4Named {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum C4Attr {
    Str(String),
    Named(C4Named),
}

/// 入れた順を保つ `Map<string, string>`。
pub type Extra = Vec<(String, String)>;

pub fn set_extra(extra: &mut Option<Extra>, key: &str, value: &str) {
    let list = extra.get_or_insert_with(Vec::new);
    match list.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value.to_string(),
        None => list.push((key.to_string(), value.to_string())),
    }
}

#[derive(Clone, Debug, Default)]
pub struct C4Element {
    pub alias: String,
    pub label: String,
    /// 別名やラベルの場所に書いた名前つきの引数。名前の項目に当てる。
    pub alias_attr: Option<C4Named>,
    pub label_attr: Option<C4Named>,
    /// 親の境界（`C4Db::boundaries` の位置）。
    pub parent: Option<usize>,
    pub type_: Option<String>,
    pub descr: Option<String>,
    pub techn: Option<String>,
    pub sprite: Option<String>,
    pub tags: Option<String>,
    pub link: Option<String>,
    pub bg_color: Option<String>,
    pub font_color: Option<String>,
    pub border_color: Option<String>,
    pub shadowing: Option<String>,
    pub shape: Option<String>,
    pub legend_text: Option<String>,
    pub legend_sprite: Option<String>,
    /// 知らない項目を決める名前つきの引数。
    pub extra: Option<Extra>,
    /// 形のとき: Mermaid の typeC4Shape（person、external_system_db、container_queue など）。
    pub kind: String,
    /// 配置のノードのとき: node、nodeL、nodeR。
    pub node_type: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct C4Rel {
    pub type_: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub from_attr: Option<C4Named>,
    pub to_attr: Option<C4Named>,
    pub label_attr: Option<C4Named>,
    pub techn: Option<String>,
    pub descr: Option<String>,
    pub sprite: Option<String>,
    pub tags: Option<String>,
    pub link: Option<String>,
    pub text_color: Option<String>,
    pub line_color: Option<String>,
    pub offset_x: Option<f64>,
    pub offset_y: Option<f64>,
    pub extra: Option<Extra>,
}
