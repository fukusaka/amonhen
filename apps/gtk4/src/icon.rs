//! サイドバーの小さなアイコン。GSK のパスで描き、文字の色に合わせる（アイコンのテーマに頼らない。
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
    Branch,
    Remote,
    Tag,
    Stash,
    Submodule,
    Folder,
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

    impl ObjectImpl for Icon {}

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
    }
    let stroke = gsk::Stroke::new(1.3);
    stroke.set_line_cap(gsk::LineCap::Round);
    stroke.set_line_join(gsk::LineJoin::Round);
    snapshot.append_stroke(&p.to_path(), &stroke, color);
    if let Some(f) = fill {
        snapshot.append_fill(&f.to_path(), gsk::FillRule::Winding, color);
    }
}
