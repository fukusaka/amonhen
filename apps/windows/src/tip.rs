//! 押せない部品のツールチップ（docs/ux.md の決まり: 押せないときはツールチップでわけを言う）。WinUI は押せない
//! 部品のツールチップを出さないので、透明な Grid で包み、ツールチップはその Grid に付ける。

use windows_reactor::*;

/// `child` を透明な Grid で包み、Grid に `tip` を付ける（押せなくてもツールチップが出る）。`column` は外の Grid
/// での列（Grid の中でなければ 0）。
pub fn holder(child: impl Into<View>, tip: impl Into<String>, column: i32) -> View {
    Grid::new()
        .grid_column(column)
        .background(Brush::Solid(Color::argb(0, 0, 0, 0)))
        .children((child.into(),))
        .tooltip(tip.into())
}
