//! History の右端の細い帯（macOS 版の `DensityStripView`）: 行の区間ごとのコミットの頻度（左半分、青）とブランチの
//! 活動量（右半分、緑）を濃さで描き、今見えている範囲を枠で示す。押す・ドラッグするとそこへ飛ぶ。
//! 密度はコア（`HistorySnapshot::density`）が裏で求める（linux で数 ms）。History か帯の高さが変わったら求め直す。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use amonhen_model::tr;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::history::HistoryView;

/// 帯の幅（macOS 版と同じ）。
const WIDTH: i32 = 8;

#[derive(Default)]
struct State {
    /// 区間ごとの [頻度, 活動量]（0〜1）。
    density: Vec<[f32; 2]>,
    /// 求めた（求めている）History の世代と区間の数。
    key: Option<(u64, usize)>,
}

/// History の右に置く帯を作る。
pub fn strip(view: &HistoryView) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(WIDTH);
    area.set_vexpand(true);
    area.set_tooltip_text(Some(&tr!(
        "Commit frequency (left) and branch activity (right). Click to jump there."
    )));
    let state: Rc<RefCell<State>> = Rc::default();
    let weak = view.downgrade();
    let st = state.clone();
    area.set_draw_func(move |area, cr, width, height| {
        let Some(view) = weak.upgrade() else { return };
        let Some((_, history)) = view.current() else {
            return;
        };
        let rows = history.graph.len();
        // 1 区間に 25 行以上入るようにする（少ないと値がばらついて縞になる）。高さ 2 px につき 1 区間まで。
        let buckets = ((height / 2) as usize).min(rows / 25).max(1);
        let key = (history.generation, buckets);
        if st.borrow().key != Some(key) {
            st.borrow_mut().key = Some(key);
            let (st, area) = (st.clone(), area.downgrade());
            glib::spawn_future_local(async move {
                let Ok(density) = gio::spawn_blocking(move || history.density(buckets)).await
                else {
                    return;
                };
                if st.borrow().key != Some(key) {
                    return;
                }
                st.borrow_mut().density = density;
                if let Some(area) = area.upgrade() {
                    area.queue_draw();
                }
            });
        }
        let (w, h) = (width as f64, height as f64);
        let state = st.borrow();
        let density = &state.density;
        if !density.is_empty() {
            let step = h / density.len() as f64;
            for (i, [frequency, activity]) in density.iter().enumerate() {
                let y = i as f64 * step;
                cr.set_source_rgba(0.21, 0.52, 0.89, *frequency as f64 * 0.75);
                cr.rectangle(0.0, y, w / 2.0, step + 0.5);
                let _ = cr.fill();
                cr.set_source_rgba(0.20, 0.66, 0.30, *activity as f64 * 0.75);
                cr.rectangle(w / 2.0, y, w / 2.0, step + 0.5);
                let _ = cr.fill();
            }
        }
        // 今見えている範囲の枠。
        if let Some(adj) = view.vadjustment()
            && adj.upper() > 0.0
        {
            let top = adj.value() / adj.upper() * h;
            let bottom = ((adj.value() + adj.page_size()) / adj.upper() * h).max(top + 3.0);
            let fg = area.color();
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.6);
            cr.set_line_width(1.0);
            cr.rectangle(0.5, top + 0.5, w - 1.0, (bottom - top - 1.0).max(1.0));
            let _ = cr.stroke();
        }
    });
    // スクロールしたら枠を、History が替わったら帯を描き直す（History が替わると表の大きさの知らせも来る）。
    let watched: Rc<Cell<bool>> = Rc::default();
    let connect = {
        let (area, view) = (area.downgrade(), view.downgrade());
        move || {
            let (Some(area), Some(view)) = (area.upgrade(), view.upgrade()) else {
                return;
            };
            if watched.replace(true) {
                return;
            }
            if let Some(adj) = view.vadjustment() {
                let a = area.downgrade();
                adj.connect_value_changed(move |_| {
                    if let Some(a) = a.upgrade() {
                        a.queue_draw();
                    }
                });
                let a = area.downgrade();
                adj.connect_changed(move |_| {
                    if let Some(a) = a.upgrade() {
                        a.queue_draw();
                    }
                });
            }
        }
    };
    area.connect_map(move |_| connect());
    let a = area.downgrade();
    view.connect_history_changed(move || {
        if let Some(a) = a.upgrade() {
            a.queue_draw();
        }
    });
    // 押す・ドラッグする: その位置の行を、表の中ほどに出す。
    let jump = {
        let view = view.downgrade();
        move |area: &gtk::DrawingArea, y: f64| {
            let Some(view) = view.upgrade() else { return };
            let rows = view.row_count();
            if rows == 0 {
                return;
            }
            let fraction = (y / area.height().max(1) as f64).clamp(0.0, 1.0);
            let row = ((fraction * rows as f64) as u32).min(rows - 1);
            view.center_row(row);
        }
    };
    let drag = gtk::GestureDrag::new();
    let (j, start) = (jump.clone(), Rc::new(Cell::new(0.0)));
    let s = start.clone();
    drag.connect_drag_begin(move |g, _, y| {
        s.set(y);
        if let Some(area) = g.widget().and_downcast::<gtk::DrawingArea>() {
            j(&area, y);
        }
    });
    drag.connect_drag_update(move |g, _, dy| {
        if let Some(area) = g.widget().and_downcast::<gtk::DrawingArea>() {
            jump(&area, start.get() + dy);
        }
    });
    area.add_controller(drag);
    area
}
