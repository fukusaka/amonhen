//! 前のフレームで組み立てた行のセル。スクロールではほとんどの行がずれるだけなので、組み立て直さずにここから
//! 写す。行のセルは、行の番号と `key`（幅など、中身を決めるもの）だけで決まるものに限る。選択の印のように
//! フレームごとに変わるものは、写した後に描く。

use ratatui_core::buffer::Cell;

#[derive(Default)]
pub struct RowCache<K> {
    /// セルの中身を決めるもの。違えば、すべて捨てる。
    key: K,
    /// 行ごとのセルの数。
    width: usize,
    /// 置き場所ごとに、入っている行（なければ `u32::MAX`）。行 `r` の置き場所は `r % rows.len()`。
    rows: Vec<u32>,
    cells: Vec<Cell>,
}

impl<K: PartialEq> RowCache<K> {
    /// `key` と大きさが前と違えば、空にする。`slots`（1 以上）は、持っておく行の数。
    pub fn prepare(&mut self, key: K, width: usize, slots: usize) {
        if self.key == key && self.width == width && self.rows.len() == slots {
            return;
        }
        self.key = key;
        self.width = width;
        self.rows.clear();
        self.rows.resize(slots, u32::MAX);
        self.cells.clear();
        self.cells.resize(slots * width, Cell::default());
    }

    /// 持っている行を捨てる（次の `prepare` で、空から始まる）。
    pub fn forget(&mut self) {
        self.rows.clear();
    }

    /// `row` のセルを `to` に写す（持っていなければ `false`）。`prepare` の後に呼ぶ。
    pub fn copy_to(&self, row: u32, to: &mut [Cell]) -> bool {
        let slot = row as usize % self.rows.len();
        if self.rows[slot] != row {
            return false;
        }
        to.clone_from_slice(&self.cells[slot * self.width..(slot + 1) * self.width]);
        true
    }

    pub fn store(&mut self, row: u32, from: &[Cell]) {
        let slot = row as usize % self.rows.len();
        self.rows[slot] = row;
        self.cells[slot * self.width..(slot + 1) * self.width].clone_from_slice(from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 持っている行は写せる。`key` か大きさが変われば、捨てる。同じ置き場所の行は、新しいほうが残る
    #[test]
    fn rows_are_kept_until_the_key_changes() {
        let cell = |ch: char| {
            let mut cell = Cell::default();
            cell.set_char(ch);
            cell
        };
        let mut cache = RowCache::default();
        cache.prepare(1u8, 2, 4);
        cache.store(5, &[cell('a'), cell('b')]);
        let mut to = [Cell::default(), Cell::default()];
        assert!(cache.copy_to(5, &mut to));
        assert_eq!((to[0].symbol(), to[1].symbol()), ("a", "b"));
        // 行 9 は行 5 と同じ置き場所。
        assert!(!cache.copy_to(9, &mut to));
        cache.store(9, &[cell('c'), cell('d')]);
        assert!(!cache.copy_to(5, &mut to));
        cache.prepare(1, 2, 4);
        assert!(cache.copy_to(9, &mut to));
        cache.prepare(2, 2, 4);
        assert!(!cache.copy_to(9, &mut to));
        cache.store(9, &[cell('c'), cell('d')]);
        cache.forget();
        cache.prepare(2, 2, 4);
        assert!(!cache.copy_to(9, &mut to));
    }
}
