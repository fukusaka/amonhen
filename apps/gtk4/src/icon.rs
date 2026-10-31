//! サイドバーと見出しのボタンの小さなアイコン（形は macOS 版の SF Symbols に寄せる）。GSK のパスで描き、文字の色に合わせる（アイコンのテーマに頼らない。
//! テーマによってブランチやタグのアイコンがないため）。

use std::cell::Cell;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

const SIZE: i32 = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    None,
    WorkingCopy,
    History,
    /// clock.arrow.circlepath
    Reflog,
    Branch,
    Remote,
    Tag,
    Stash,
    Submodule,
    Folder,
    /// arrow.down.circle
    Fetch,
    /// arrow.down.to.line.circle
    Pull,
    /// arrow.up.circle
    Push,
    /// arrow.triangle.merge
    Merge,
    /// arrow.triangle.pull
    Rebase,
    /// tray.and.arrow.down
    StashPush,
    /// tray.and.arrow.up
    StashPop,
    /// arrow.clockwise
    Refresh,
    /// arrow.uturn.backward.circle
    Undo,
    /// magnifyingglass
    Search,
    /// line.3.horizontal.decrease
    Filter,
    /// list.bullet
    List,
    /// list.bullet.indent
    Tree,
    /// doc.text
    Doc,
    /// arrow.left.arrow.right（Range Diff）
    RangeDiff,
    /// slider.horizontal.3（History の表示のメニュー）
    Sliders,
    /// location（Go to HEAD）
    Location,
    /// star.fill（お気に入りのブランチ）
    Star,
    /// arrow.turn.down.right（ほかのブランチの上に積んだブランチ）
    Stacked,
    /// icloud.slash（上流がリモートで消えた）
    CloudSlash,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Icon {
        pub kind: Cell<Kind>,
        /// 文字の色ではなく強調の色で描く（チェックアウト中のブランチ）。
        pub accent: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Icon {
        const NAME: &'static str = "AmonhenIcon";
        type Type = super::Icon;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Icon {
        fn constructed(&self) {
            self.parent_constructed();
            // ボタンや行の中で、上下左右の真ん中に置く。
            self.obj().set_valign(gtk::Align::Center);
            self.obj().set_halign(gtk::Align::Center);
        }
    }

    impl WidgetImpl for Icon {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (SIZE, SIZE, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let fg = self.obj().color();
            let color = if self.accent.get() {
                gdk::RGBA::new(0.21, 0.52, 0.89, 1.0)
            } else {
                gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.7)
            };
            draw(snapshot, self.kind.get(), &color);
        }
    }
}

glib::wrapper! {
    pub struct Icon(ObjectSubclass<imp::Icon>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Icon {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl Icon {
    pub fn set(&self, kind: Kind, accent: bool) {
        self.imp().kind.set(kind);
        self.imp().accent.set(accent);
        self.set_visible(kind != Kind::None);
        self.queue_draw();
    }
}

/// 雲の輪郭（icloud）。
fn cloud(p: &gsk::PathBuilder) {
    p.move_to(4.5, 12.5);
    p.line_to(11.8, 12.5);
    p.svg_arc_to(2.4, 2.4, 0.0, false, false, 12.2, 7.7);
    p.svg_arc_to(3.6, 3.6, 0.0, false, false, 5.3, 7.0);
    p.svg_arc_to(2.8, 2.8, 0.0, false, false, 4.5, 12.5);
    p.close();
}

/// 16×16 の中に描く。
fn draw(snapshot: &gtk::Snapshot, kind: Kind, color: &gdk::RGBA) {
    let p = gsk::PathBuilder::new();
    let point = |x: f32, y: f32| graphene::Point::new(x, y);
    let mut fill = None;
    match kind {
        Kind::None => return,
        Kind::WorkingCopy => {
            // 鉛筆。
            p.move_to(3.0, 13.0);
            p.line_to(3.0, 10.5);
            p.line_to(10.5, 3.0);
            p.line_to(13.0, 5.5);
            p.line_to(5.5, 13.0);
            p.close();
            p.move_to(9.0, 4.5);
            p.line_to(11.5, 7.0);
        }
        Kind::History => {
            // 時計。
            p.add_circle(&point(8.0, 8.0), 5.5);
            p.move_to(8.0, 4.8);
            p.line_to(8.0, 8.0);
            p.line_to(10.5, 9.5);
        }
        Kind::Reflog => {
            // 左上を開けた円と、その端の矢じり（左回り）、中に時計の針。
            p.move_to(3.0, 8.0);
            p.svg_arc_to(5.0, 5.0, 0.0, true, false, 6.5, 3.2);
            p.move_to(7.0, 1.2);
            p.line_to(4.8, 3.4);
            p.line_to(7.2, 5.4);
            p.move_to(8.0, 5.5);
            p.line_to(8.0, 8.0);
            p.line_to(10.0, 9.3);
        }
        Kind::Branch => {
            // 幹と、合流してくる枝。
            p.add_circle(&point(5.0, 3.5), 1.6);
            p.add_circle(&point(5.0, 12.5), 1.6);
            p.add_circle(&point(11.0, 4.5), 1.6);
            p.move_to(5.0, 5.1);
            p.line_to(5.0, 10.9);
            p.move_to(11.0, 6.1);
            p.cubic_to(11.0, 9.0, 5.0, 8.0, 5.0, 10.9);
        }
        Kind::Remote => {
            // 地球。
            p.add_circle(&point(8.0, 8.0), 5.5);
            p.move_to(2.5, 8.0);
            p.line_to(13.5, 8.0);
            p.move_to(8.0, 2.5);
            p.quad_to(4.0, 8.0, 8.0, 13.5);
            p.quad_to(12.0, 8.0, 8.0, 2.5);
        }
        Kind::Tag => {
            p.move_to(2.5, 2.5);
            p.line_to(8.0, 2.5);
            p.line_to(13.5, 8.0);
            p.line_to(8.0, 13.5);
            p.line_to(2.5, 8.0);
            p.close();
            let hole = gsk::PathBuilder::new();
            hole.add_circle(&point(5.5, 5.5), 1.1);
            fill = Some(hole);
        }
        Kind::Stash => {
            // 受け皿。
            p.move_to(2.5, 8.5);
            p.line_to(2.5, 13.0);
            p.line_to(13.5, 13.0);
            p.line_to(13.5, 8.5);
            p.move_to(2.5, 8.5);
            p.line_to(5.5, 8.5);
            p.line_to(6.5, 10.0);
            p.line_to(9.5, 10.0);
            p.line_to(10.5, 8.5);
            p.line_to(13.5, 8.5);
            p.move_to(4.0, 8.5);
            p.line_to(5.0, 3.0);
            p.line_to(11.0, 3.0);
            p.line_to(12.0, 8.5);
        }
        Kind::Submodule => {
            p.add_rect(&graphene::Rect::new(2.5, 3.5, 11.0, 9.5));
            p.move_to(2.5, 6.5);
            p.line_to(13.5, 6.5);
            p.move_to(6.5, 9.0);
            p.line_to(9.5, 9.0);
        }
        Kind::Folder => {
            p.move_to(2.5, 4.0);
            p.line_to(6.5, 4.0);
            p.line_to(8.0, 5.5);
            p.line_to(13.5, 5.5);
            p.line_to(13.5, 12.5);
            p.line_to(2.5, 12.5);
            p.close();
        }
        Kind::Fetch | Kind::Pull | Kind::Push => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            let (top, bottom) = match kind {
                Kind::Pull => (4.3, 9.6),
                _ => (4.5, 11.5),
            };
            if kind == Kind::Push {
                p.move_to(8.0, 11.5);
                p.line_to(8.0, 4.5);
                p.move_to(5.5, 7.0);
                p.line_to(8.0, 4.5);
                p.line_to(10.5, 7.0);
            } else {
                p.move_to(8.0, top);
                p.line_to(8.0, bottom);
                p.move_to(5.5, bottom - 2.5);
                p.line_to(8.0, bottom);
                p.line_to(10.5, bottom - 2.5);
            }
            if kind == Kind::Pull {
                p.move_to(5.0, 11.8);
                p.line_to(11.0, 11.8);
            }
        }
        Kind::Merge => {
            // 2 本が合流して 1 本になる。
            p.move_to(4.0, 2.5);
            p.line_to(4.0, 5.0);
            p.cubic_to(4.0, 8.0, 8.0, 8.0, 8.0, 11.0);
            p.line_to(8.0, 14.0);
            p.move_to(12.0, 2.5);
            p.line_to(12.0, 5.0);
            p.cubic_to(12.0, 8.0, 8.0, 8.0, 8.0, 11.0);
            p.move_to(9.8, 4.3);
            p.line_to(12.0, 2.3);
            p.line_to(14.2, 4.3);
        }
        Kind::Rebase => {
            // 枝を幹の先へ載せ替える。
            p.move_to(4.5, 14.0);
            p.line_to(4.5, 2.5);
            p.move_to(11.5, 14.0);
            p.line_to(11.5, 9.0);
            p.cubic_to(11.5, 6.0, 8.0, 5.0, 6.0, 4.0);
            p.move_to(6.5, 6.6);
            p.line_to(5.6, 3.8);
            p.line_to(8.5, 3.0);
        }
        Kind::StashPush | Kind::StashPop => {
            p.move_to(2.5, 9.0);
            p.line_to(2.5, 13.5);
            p.line_to(13.5, 13.5);
            p.line_to(13.5, 9.0);
            p.move_to(2.5, 9.0);
            p.line_to(5.5, 9.0);
            p.line_to(6.5, 10.5);
            p.line_to(9.5, 10.5);
            p.line_to(10.5, 9.0);
            p.line_to(13.5, 9.0);
            if kind == Kind::StashPush {
                p.move_to(8.0, 1.8);
                p.line_to(8.0, 7.5);
                p.move_to(5.8, 5.3);
                p.line_to(8.0, 7.5);
                p.line_to(10.2, 5.3);
            } else {
                p.move_to(8.0, 7.5);
                p.line_to(8.0, 1.8);
                p.move_to(5.8, 4.0);
                p.line_to(8.0, 1.8);
                p.line_to(10.2, 4.0);
            }
        }
        Kind::Refresh => {
            // 右上を開けた円と、その端の矢じり。
            p.move_to(13.0, 8.0);
            p.svg_arc_to(5.0, 5.0, 0.0, true, true, 9.5, 3.2);
            p.move_to(9.0, 1.2);
            p.line_to(11.2, 3.4);
            p.line_to(8.8, 5.4);
        }
        Kind::List | Kind::Tree => {
            let indent = if kind == Kind::Tree { 3.0 } else { 0.0 };
            for (i, y) in [4.0f32, 8.0, 12.0].iter().enumerate() {
                let x = if i == 0 { 0.0 } else { indent };
                let dot = gsk::PathBuilder::new();
                dot.add_circle(&point(3.0 + x, *y), 0.9);
                p.add_path(&dot.to_path());
                p.move_to(6.0 + x, *y);
                p.line_to(13.5, *y);
            }
        }
        Kind::Doc => {
            p.move_to(4.0, 2.5);
            p.line_to(9.5, 2.5);
            p.line_to(12.5, 5.5);
            p.line_to(12.5, 13.5);
            p.line_to(4.0, 13.5);
            p.close();
            p.move_to(6.0, 8.0);
            p.line_to(10.5, 8.0);
            p.move_to(6.0, 10.5);
            p.line_to(10.5, 10.5);
        }
        Kind::RangeDiff => {
            // 右向きの矢印（上）と左向きの矢印（下）。
            p.move_to(2.5, 5.5);
            p.line_to(13.0, 5.5);
            p.move_to(10.5, 3.0);
            p.line_to(13.0, 5.5);
            p.line_to(10.5, 8.0);
            p.move_to(13.5, 10.5);
            p.line_to(3.0, 10.5);
            p.move_to(5.5, 8.0);
            p.line_to(3.0, 10.5);
            p.line_to(5.5, 13.0);
        }
        Kind::Sliders => {
            // 3 本の横線と、それぞれのつまみ。
            for (y, knob) in [(4.0f32, 10.5f32), (8.0, 5.5), (12.0, 9.0)] {
                p.move_to(2.5, y);
                p.line_to(13.5, y);
                let dot = gsk::PathBuilder::new();
                dot.add_circle(&point(knob, y), 1.6);
                fill.get_or_insert_with(gsk::PathBuilder::new)
                    .add_path(&dot.to_path());
            }
        }
        Kind::Location => {
            // 右上を指す矢じり（位置を示す矢印）。
            p.move_to(13.5, 2.5);
            p.line_to(2.5, 7.3);
            p.line_to(8.0, 8.0);
            p.line_to(8.7, 13.5);
            p.close();
        }
        Kind::Star => {
            // 塗った五つの角の星。
            let star = gsk::PathBuilder::new();
            for i in 0..10 {
                let r = if i % 2 == 0 { 6.3 } else { 2.7 };
                let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
                let (x, y) = (8.0 + r * a.cos(), 8.4 + r * a.sin());
                if i == 0 {
                    star.move_to(x, y);
                } else {
                    star.line_to(x, y);
                }
            }
            star.close();
            // to_path で空になるので、作ったパスを輪郭と塗りの両方に入れる。
            let star = star.to_path();
            p.add_path(&star);
            let solid = gsk::PathBuilder::new();
            solid.add_path(&star);
            fill = Some(solid);
        }
        Kind::Stacked => {
            // 下に折れて右へ向かう矢印。
            p.move_to(4.0, 2.5);
            p.line_to(4.0, 9.5);
            p.line_to(12.5, 9.5);
            p.move_to(10.0, 7.0);
            p.line_to(12.5, 9.5);
            p.line_to(10.0, 12.0);
        }
        Kind::CloudSlash => {
            cloud(&p);
            p.move_to(2.5, 2.5);
            p.line_to(13.5, 13.5);
        }
        Kind::Filter => {
            p.move_to(2.5, 4.5);
            p.line_to(13.5, 4.5);
            p.move_to(4.5, 8.0);
            p.line_to(11.5, 8.0);
            p.move_to(6.5, 11.5);
            p.line_to(9.5, 11.5);
        }
        Kind::Search => {
            p.add_circle(&point(6.8, 6.8), 4.3);
            p.move_to(10.0, 10.0);
            p.line_to(13.8, 13.8);
        }
        Kind::Undo => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(10.8, 11.0);
            p.line_to(10.8, 8.0);
            p.svg_arc_to(2.4, 2.4, 0.0, false, false, 6.0, 8.0);
            p.line_to(6.0, 10.0);
            p.move_to(4.3, 8.4);
            p.line_to(6.0, 10.2);
            p.line_to(7.7, 8.4);
        }
    }
    let stroke = gsk::Stroke::new(1.3);
    stroke.set_line_cap(gsk::LineCap::Round);
    stroke.set_line_join(gsk::LineJoin::Round);
    snapshot.append_stroke(&p.to_path(), &stroke, color);
    if let Some(f) = fill {
        snapshot.append_fill(&f.to_path(), gsk::FillRule::Winding, color);
    }
}
