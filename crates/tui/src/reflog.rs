//! Reflog: HEAD（か、選んだブランチ）が動いた記録を新しい順に。選んだ行のコミットの詳細と差分は、History と同じ枠に出す。

use amonhen_model::{Oid, tr};
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::diff::DIM;
use crate::history::{ago, fit};
use crate::loader::Request;

const SELECTED: Color = Color::Indexed(238);
const SELECTED_UNFOCUSED: Color = Color::Indexed(236);
/// 「HEAD@{12}」の幅（これより狭くはしない）。
const SELECTOR_WIDTH: u16 = 10;
const DATE_WIDTH: u16 = 4;

struct Entry {
    selector: String,
    new: Option<Oid>,
    time: i64,
    /// 何をしたか（「commit: …」「checkout: moving from … to …」）。
    message: String,
    /// History にある（ref から辿れる）。なければ、Reflog にだけ残っているコミット。
    reachable: bool,
}

#[derive(Default)]
pub struct Reflog {
    /// どの ref のものか（`None` は HEAD）。
    name: Option<String>,
    entries: Vec<Entry>,
    loaded: bool,
    loading: bool,
    /// 読み直しが要る。
    stale: bool,
    selected: usize,
    top: usize,
    height: u16,
}

impl Reflog {
    /// 出す ref を替える（`None` は HEAD）。替わったら、中身は読み直すまで空。
    pub fn show(&mut self, name: Option<String>) {
        if name != self.name {
            *self = Self {
                name,
                height: self.height,
                ..Self::default()
            };
        }
    }

    /// 出している ref（HEAD なら `None`）。
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// 読み直す（次の `request` で頼む）。
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    /// 画面に出ているなら、頼むことがあればその中身。
    pub fn request(&mut self, visible: bool) -> Option<Request> {
        if !visible || self.loading || !(self.stale || !self.loaded) {
            return None;
        }
        self.stale = false;
        self.loading = true;
        Some(Request::Reflog {
            name: self.name.clone(),
        })
    }

    /// `name` の ref の `reflog` の答え（`info.entries`）。待つ間に出す ref を替えていたら捨てて、頼み直す。
    pub fn set(
        &mut self,
        name: Option<String>,
        reply: std::result::Result<Value, String>,
    ) -> Option<String> {
        self.loading = false;
        if name != self.name {
            return None;
        }
        self.loaded = true;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => return Some(error),
        };
        self.entries = reply["info"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| Entry {
                selector: e["selector"].as_str().unwrap_or("").to_owned(),
                new: e["new"].as_str().and_then(Oid::from_hex),
                time: e["time"].as_i64().unwrap_or(0),
                message: e["message"].as_str().unwrap_or("").to_owned(),
                reachable: e["reachable"] == true,
            })
            .collect();
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.clamp_top();
        None
    }

    /// 選んでいる行のコミット。
    pub fn selected_id(&self) -> Option<Oid> {
        self.entries.get(self.selected)?.new
    }

    /// 選んでいる行のコミットが History にあるか。
    pub fn selected_reachable(&self) -> bool {
        self.entries.get(self.selected).is_some_and(|e| e.reachable)
    }

    pub fn set_height(&mut self, height: u16) {
        self.height = height;
        self.clamp_top();
    }

    fn clamp_top(&mut self) {
        let height = usize::from(self.height.max(1));
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + height {
            self.top = self.selected + 1 - height;
        }
        self.top = self.top.min(self.entries.len().saturating_sub(height));
    }

    pub fn page(&self) -> i64 {
        i64::from(self.height.max(2)) - 1
    }

    pub fn move_selection(&mut self, delta: i64) -> bool {
        self.select((self.selected as i64 + delta).max(0) as usize)
    }

    pub fn select(&mut self, index: usize) -> bool {
        let index = index.min(self.entries.len().saturating_sub(1));
        let changed = index != self.selected;
        self.selected = index;
        self.clamp_top();
        changed
    }

    pub fn scroll(&mut self, delta: i64) -> bool {
        let last = self
            .entries
            .len()
            .saturating_sub(usize::from(self.height.max(1)));
        let top = (self.top as i64 + delta).clamp(0, last as i64) as usize;
        let changed = top != self.top;
        self.top = top;
        changed
    }

    pub fn click(&mut self, line: u16) -> bool {
        let index = self.top + usize::from(line);
        index < self.entries.len() && self.select(index)
    }

    pub fn position(&self) -> (u32, u32) {
        let count = self.entries.len() as u32;
        ((self.selected as u32 + 1).min(count), count)
    }

    pub fn draw(&self, area: Rect, buf: &mut Buffer, now: i64, focused: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.entries.is_empty() {
            let text = match self.loaded {
                true => tr!("The reflog is empty"),
                false => tr!("Loading…"),
            };
            buf.set_stringn(
                area.x + 1,
                area.y,
                text,
                usize::from(area.width.saturating_sub(1)),
                Style::new().fg(DIM),
            );
            return;
        }
        let dim = Style::new().fg(DIM);
        let date_x = area.right().saturating_sub(DATE_WIDTH);
        // ブランチの名前は長いことがある（「HEAD@{12}」より）。見えている行に合わせ、幅の 3 分の 1 まで。
        let selector_width = (self.entries.iter().skip(self.top))
            .take(usize::from(area.height))
            .map(|e| e.selector.width() as u16)
            .max()
            .unwrap_or(0)
            .clamp(SELECTOR_WIDTH, (area.width / 3).max(SELECTOR_WIDTH));
        let text_x = area.x + selector_width + 1;
        for (y, (index, entry)) in (area.y..area.bottom()).zip(
            self.entries
                .iter()
                .enumerate()
                .skip(self.top)
                .take(usize::from(area.height)),
        ) {
            buf.set_stringn(
                area.x,
                y,
                fit(&entry.selector, usize::from(selector_width)),
                usize::from(area.width),
                dim,
            );
            if text_x + 1 < date_x {
                // History にないコミットの行は薄く。
                let style = match entry.reachable {
                    true => Style::new(),
                    false => dim,
                };
                let room = usize::from(date_x - text_x - 1);
                buf.set_stringn(text_x, y, fit(&entry.message, room), room, style);
                let date = ago(now, entry.time);
                let pad = DATE_WIDTH.saturating_sub(date.width() as u16);
                buf.set_string(date_x + pad, y, date, dim);
            }
            if index == self.selected {
                buf.set_style(
                    Rect::new(area.x, y, area.width, 1),
                    Style::new().bg(if focused {
                        SELECTED
                    } else {
                        SELECTED_UNFOCUSED
                    }),
                );
            }
        }
    }
}
