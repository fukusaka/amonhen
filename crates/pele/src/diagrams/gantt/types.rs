//! gantt の形（pele の `src/diagrams/gantt/types.ts`）。モデルは `db::GanttDb` そのもの。

/// 日付は時刻の値（`js_date`）。`None` は JS の `undefined`、`Some(NaN)` は不正な日付。
#[derive(Clone, Debug)]
pub struct GanttTask {
    pub id: String,
    /// タスクのラベル（Mermaid の名前のまま）。
    pub task: String,
    pub section: String,
    /// `get_tasks` が予定を組むまでは無い。解けないものを指すタスクも無いまま。
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
    /// 除く日がタスクの終わりを押し出したときの、棒の終わり。終わりを日付で書いたときは None。
    pub render_end_time: Option<f64>,
    pub manual_end_time: bool,
    pub processed: bool,
    pub active: bool,
    pub done: bool,
    pub crit: bool,
    pub milestone: bool,
    pub vert: bool,
    /// 書いた順の行の番号。縦の印は行を取らず -1。
    pub order: i64,
    pub classes: Vec<String>,
    pub prev_task_id: Option<String>,
    pub raw: Raw,
}

#[derive(Clone, Debug)]
pub struct Raw {
    pub data: String,
    pub start: Option<String>,
    pub end: String,
}
