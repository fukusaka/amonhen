//! フレームの出力。端末に出ているセルを持ち、描き直した所だけを比べて、変わったセルだけを書く。
//!
//! ratatui の `Terminal` は、フレームごとにすべてのセルを消して組み立て直し、すべてを比べる。ここでは描き直す
//! 所（`areas`）の外のセルに触らないので、動かなかったペインは組み立ても比べもしない。

use std::io::Write;

use ratatui_core::backend::{Backend, ClearType};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_crossterm::CrosstermBackend;
use ratatui_crossterm::crossterm::queue;
use ratatui_crossterm::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use unicode_width::UnicodeWidthStr;

pub struct Canvas<W: Write> {
    backend: CrosstermBackend<W>,
    /// 端末に出ているセル。
    shown: Buffer,
    /// 次に出すセル。描き直す所の外は `shown` と同じ。
    next: Buffer,
    /// 変わったセルの位置（フレームごとに使い回す）。
    changed: Vec<usize>,
    /// 端末にまだ何も出していない（次のフレームはすべて書く）。
    blank: bool,
    cursor_shown: bool,
}

impl<W: Write> Canvas<W> {
    pub fn new(out: W) -> Self {
        Self {
            backend: CrosstermBackend::new(out),
            shown: Buffer::default(),
            next: Buffer::default(),
            changed: Vec::new(),
            blank: true,
            // 端末を取るときに隠してある。
            cursor_shown: false,
        }
    }

    /// 出力先（端末の設定を替える列を書くのに）。
    pub fn out(&mut self) -> &mut CrosstermBackend<W> {
        &mut self.backend
    }

    /// 端末に出ているものを当てにしない（次のフレームは、消してからすべて書く）。
    pub fn forget(&mut self) {
        self.blank = true;
    }

    /// 端末に出ているセル。
    pub fn shown(&self) -> &Buffer {
        &self.shown
    }

    /// 1 フレームを出す。`areas` のセルを消してから `render` に描かせ（`areas` の外には描かないこと）、前の
    /// フレームと違うセルだけを書く。`render` が返すのは、カーソルを置く位置（なければ隠す）。大きさが変わったか、
    /// まだ何も出していなければ、`areas` によらずすべてを描き直す（`render` には `true` を渡す）。対応する端末では
    /// まとめて表示されるよう、synchronized update で包む。
    pub fn draw(
        &mut self,
        size: (u16, u16),
        areas: &[Rect],
        render: impl FnOnce(&mut Buffer, bool) -> Option<(u16, u16)>,
    ) -> std::io::Result<()> {
        let all = Rect::new(0, 0, size.0, size.1);
        queue!(self.backend, BeginSynchronizedUpdate)?;
        if self.next.area != all {
            self.shown = Buffer::empty(all);
            self.next = Buffer::empty(all);
            self.blank = true;
        }
        let everything = self.blank;
        if everything {
            self.backend.clear_region(ClearType::All)?;
            self.shown.reset();
            self.blank = false;
        }
        let whole = [all];
        let areas = if everything { &whole[..] } else { areas };
        let width = usize::from(all.width);
        for area in areas {
            let area = area.intersection(all);
            for y in area.y..area.bottom() {
                let start = usize::from(y) * width + usize::from(area.x);
                for cell in &mut self.next.content[start..start + usize::from(area.width)] {
                    cell.reset();
                }
            }
        }
        let cursor = render(&mut self.next, everything);

        self.changed.clear();
        for area in areas {
            let area = area.intersection(all);
            for y in area.y..area.bottom() {
                let start = usize::from(y) * width + usize::from(area.x);
                // 全角の文字の後ろのセルは書かない。全角だった所が半角になったら、後ろのセルも書き直す
                // （ratatui の `Buffer::diff` と同じ。セルごとの比べ方の指定は使っていないので、見ない）。
                let mut skip = 0;
                let mut invalidated = 0;
                for i in start..start + usize::from(area.width) {
                    let (current, previous) = (&self.next.content[i], &mut self.shown.content[i]);
                    let differs = current != previous;
                    if (differs || invalidated > 0) && skip == 0 {
                        self.changed.push(i);
                    }
                    let current_width = cells(current.symbol());
                    skip = current_width.saturating_sub(1);
                    invalidated = current_width
                        .max(cells(previous.symbol()))
                        .max(invalidated)
                        .saturating_sub(1);
                    // 出すとおりに覚える（全角の後ろのセルも）。
                    if differs {
                        previous.clone_from(current);
                    }
                }
            }
        }
        let next = &self.next;
        self.backend.draw(
            self.changed
                .iter()
                .map(|&i| ((i % width) as u16, (i / width) as u16, &next.content[i])),
        )?;
        match cursor {
            Some(at) => {
                if !self.cursor_shown {
                    self.backend.show_cursor()?;
                    self.cursor_shown = true;
                }
                self.backend.set_cursor_position(at)?;
            }
            None if self.cursor_shown => {
                self.backend.hide_cursor()?;
                self.cursor_shown = false;
            }
            None => {}
        }
        queue!(self.backend, EndSynchronizedUpdate)?;
        Backend::flush(&mut self.backend)
    }
}

/// 文字が使うセルの数（ASCII の 1 文字は数えるまでもない）。
fn cells(symbol: &str) -> usize {
    match symbol.len() {
        1 => 1,
        _ => symbol.width(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use ratatui_core::style::Style;

    use super::*;

    /// 書かれたものを、後から読める出力先。
    #[derive(Clone, Default)]
    struct Written(Rc<RefCell<Vec<u8>>>);

    impl Write for Written {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Written {
        /// ここまでに書かれたもの（読んだら空にする）。
        fn take(&self) -> String {
            String::from_utf8(std::mem::take(&mut *self.0.borrow_mut())).unwrap()
        }
    }

    fn text(canvas: &Canvas<Written>) -> String {
        canvas.shown().content.iter().map(|c| c.symbol()).collect()
    }

    /// 描き直す所の外は、描かなくても残る。変わらなければ、文字を書かない
    #[test]
    fn untouched_areas_stay_and_cost_nothing() {
        let out = Written::default();
        let mut canvas = Canvas::new(out.clone());
        canvas
            .draw((6, 1), &[], |buf, all| {
                assert!(all);
                buf.set_string(0, 0, "abcdef", Style::new());
                None
            })
            .unwrap();
        assert_eq!(text(&canvas), "abcdef");
        // 右の 3 セルだけを描き直す。
        out.take();
        canvas
            .draw((6, 1), &[Rect::new(3, 0, 3, 1)], |buf, all| {
                assert!(!all);
                buf.set_string(3, 0, "dXf", Style::new());
                None
            })
            .unwrap();
        assert_eq!(text(&canvas), "abcdXf");
        let written = out.take();
        assert_eq!(written.matches('X').count(), 1);
        assert!(
            !written.contains('d') && !written.contains('a'),
            "{written:?}"
        );
    }

    /// 忘れさせたら、同じ中身でもすべて書き直す
    #[test]
    fn forgetting_writes_everything_again() {
        let out = Written::default();
        let mut canvas = Canvas::new(out.clone());
        let draw = |canvas: &mut Canvas<Written>| {
            canvas
                .draw((6, 1), &[Rect::new(0, 0, 6, 1)], |buf, _| {
                    buf.set_string(0, 0, "abcdef", Style::new());
                    None
                })
                .unwrap();
        };
        draw(&mut canvas);
        out.take();
        draw(&mut canvas);
        assert!(!out.take().contains('a'));
        canvas.forget();
        draw(&mut canvas);
        assert!(out.take().contains("abcdef"));
    }

    /// 全角の文字が半角になったら、後ろのセルも書き直す。大きさが変わったら、すべて描き直す
    #[test]
    fn wide_characters_and_resizing() {
        let out = Written::default();
        let mut canvas = Canvas::new(out.clone());
        let area = [Rect::new(0, 0, 4, 1)];
        canvas
            .draw((4, 1), &area, |buf, _| {
                buf.set_string(0, 0, "日x", Style::new());
                None
            })
            .unwrap();
        out.take();
        canvas
            .draw((4, 1), &area, |buf, _| {
                buf.set_string(0, 0, "a x", Style::new());
                None
            })
            .unwrap();
        let written = out.take();
        // 「日」の後ろだったセル（今は空白）も書く。
        assert!(written.contains("a "), "{written:?}");
        canvas
            .draw((5, 1), &[], |buf, all| {
                assert!(all);
                buf.set_string(0, 0, "hello", Style::new());
                Some((1, 0))
            })
            .unwrap();
        assert_eq!(text(&canvas), "hello");
    }
}
