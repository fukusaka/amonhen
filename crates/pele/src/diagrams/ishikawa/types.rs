//! ishikawa の形（pele の `src/diagrams/ishikawa/types.ts`）。

/// ノード。根は結果で、その子は原因の分類、その子は原因、と続く。
#[derive(Clone, Debug)]
pub struct IshikawaNode {
    pub text: String,
    /// 子の番号（`IshikawaDb::nodes` の位置）。
    pub children: Vec<usize>,
}
