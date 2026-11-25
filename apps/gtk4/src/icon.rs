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
    /// square.stack.3d.down.right（subtree）
    Subtree,
    Folder,
    /// arrow.down.circle
    Fetch,
    /// arrow.down.to.line.circle
    Pull,
    /// arrow.up.circle
    Push,
    /// sidebar.left（サイドバーの切り替え）
    Sidebar,
    /// icloud.and.arrow.up（上流のないブランチの Push、Publish）
    Publish,
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
    /// arrow.up.arrow.down（上流より先・遅れ）
    UpDown,
    /// folder.fill（パスを選ぶシートのフォルダ）
    FolderFill,
    /// person.crop.circle（設定の Accounts）
    Person,
    /// paintbrush（設定の Appearance）
    Brush,
    /// swatchpalette（設定の Theme）
    Palette,
    /// speedometer（設定の Performance）
    Speedometer,
    /// apple.terminal（パスを選ぶシートの実行ファイル）
    Terminal,
    /// questionmark.square.dashed（パスを選ぶシートの、種類の分からないもの）
    QuestionSquare,
    /// slider.horizontal.3（History の表示のメニュー）
    Sliders,
    /// location（Go to HEAD）
    Location,
    /// 塗った丸から枝を抜いたもの（サイドバーの今のブランチ。macOS 版も同じ形を描く）
    BranchDisc,
    /// star.fill（お気に入りのブランチ）
    Star,
    /// arrow.turn.down.right（ほかのブランチの上に積んだブランチ）
    Stacked,
    /// icloud.slash（上流がリモートで消えた）
    CloudSlash,
    /// exclamationmark.icloud（リモートに同じ名前の別のタグがある）
    CloudAlert,
    /// house（メインの worktree）
    House,
    /// checkmark.circle（変更のない worktree をまとめた行）
    CheckCircle,
    /// gearshape（リポジトリの設定）
    Gear,
    /// exclamationmark.triangle（コンフリクト）
    Warning,
    /// waveform.path.ecg（Activity）
    Activity,
    /// plus.circle（ブランチの切り替えで、新しいブランチを作る行）
    PlusCircle,
    /// pencil.circle（書き込む先の worktree）
    PencilCircle,
    /// checkmark.seal（署名を確かめた。署名の印）
    Seal,
    /// xmark.seal（不正な署名・失効した鍵）
    SealCross,
    /// questionmark.circle（鍵がない・確かめられない・署名者が違う）
    Question,
    /// checkmark.seal.fill（署名を確かめ、鍵を強く信用している）
    SealFill,
    /// xmark.seal.fill（不正な署名）
    SealCrossFill,
    /// checkmark.shield（署名は正しいが、鍵の信用度が決まっていない）
    Shield,
    /// exclamationmark.shield.fill（失効した鍵）
    ShieldAlertFill,
    /// person.crop.circle.badge.questionmark（署名した人がタガーと違う）
    PersonQuestion,
    /// clock.badge.exclamationmark（期限の切れた鍵）
    ClockAlert,
    /// exclamationmark.circle（この git では確かめられない）
    ExclamationCircle,
    /// sparkles（AI の解説・提案）
    Sparkles,
    /// character.bubble（AI の翻訳）
    Bubble,
    /// scissors（shallow clone: 古い履歴を取得していない）
    Scissors,
    /// icloud.and.arrow.down（partial clone: 中身は必要なときに取る）
    CloudDown,
    /// folder.badge.minus（sparse checkout: 一部のフォルダだけ取り出している）
    FolderMinus,
    /// circle.dashed（submodule を取り出していない）
    CircleDashed,
    /// pencil（submodule の中に変更がある）
    Pencil,
    /// pencil.and.list.clipboard（History の編集モード）
    EditList,
    /// xmark.octagon.fill（ステータスバーの失敗）
    XOctagonFill,
    /// stop.circle（ステータスバーの取り消した操作）
    StopCircle,
    /// bolt.horizontal.circle（ステータスバーの切れたつながり）
    BoltCircle,
    /// checkmark.circle.fill（CI が通った。CI の吹き出しの見出し）
    CheckCircleFill,
    /// xmark.circle.fill（CI が失敗した）
    XCircleFill,
    /// circle.fill（CI が走っている）
    CircleFill,
    /// circle（CI がない）
    Circle,
    /// slash.circle（CI のチェックを取り消した）
    SlashCircle,
    /// arrow.up.left.and.arrow.down.right（詳細を広げる）
    Expand,
    /// arrow.down.right.and.arrow.up.left（広げた詳細を戻す）
    Restore,
    /// rectangle.expand.vertical（差分をすべて開く）
    ExpandVertical,
    /// rectangle.compress.vertical（差分をすべて畳む）
    CompressVertical,
    /// chevron.left（すべてのファイルの、木に戻る）
    ChevronLeft,
    /// rectangle.split.2x1（画像を並べる）
    TwoUp,
    /// rectangle.righthalf.inset.filled（画像のスワイプ）
    SwipeHalf,
    /// circle.lefthalf.filled（画像のオニオンスキン）
    HalfCircle,
    /// plus.forwardslash.minus（画像の違うピクセル）
    PlusMinus,
    /// space（差分で無視する空白）
    Space,
    /// eye（その時点のファイルをプレビューで開く）
    Eye,
    /// minus.magnifyingglass（画像を縮める）
    ZoomOut,
    /// plus.magnifyingglass（画像を広げる）
    ZoomIn,
    /// arrow.up.left.and.down.right.magnifyingglass（画像を枠に合わせる）
    ZoomFit,
    /// checkerboard.rectangle（透けた絵の下の市松模様の明暗）
    Checkerboard,
    /// arrow.turn.down.left（差分の長い行を折り返す）
    WrapLines,
    /// 共通のアイコンの repositories（Repositories の画面）
    Repositories,
    /// 共通のアイコンの new-branch
    NewBranch,
    /// 共通のアイコンの editor（エディタで開く）
    Editor,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Icon {
        pub kind: Cell<Kind>,
        /// 文字の色ではなく強調の色で描く（チェックアウト中のブランチ）。
        pub accent: Cell<bool>,
        /// 描く大きさ（0 なら 16）。形は 16×16 で決めて、拡げて描く。
        pub size: Cell<i32>,
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
            let size = self.size();
            (size, size, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let fg = self.obj().color();
            let obj = self.obj();
            let color = if self.accent.get() {
                gdk::RGBA::new(0.21, 0.52, 0.89, 1.0)
            } else if obj.has_css_class("warning") {
                // 注意（オレンジ）と、済んだこと（緑）。テーマの色はラベルにしか付かないので、ここで決める。
                gdk::RGBA::new(0.96, 0.47, 0.0, 1.0)
            } else if obj.has_css_class("success") {
                gdk::RGBA::new(0.15, 0.64, 0.25, 1.0)
            } else if obj.has_css_class("error") {
                gdk::RGBA::new(0.90, 0.25, 0.25, 1.0)
            } else if obj.has_css_class("caution") {
                // 黄色（署名は正しいが、鍵の信用度が決まっていないなど）。
                gdk::RGBA::new(0.85, 0.68, 0.0, 1.0)
            } else if shared(self.kind.get()).is_some() {
                // 共通のアイコンは文字の色のまま（Adwaita の記号のアイコンと同じ濃さ）。
                fg
            } else {
                gdk::RGBA::new(fg.red(), fg.green(), fg.blue(), fg.alpha() * 0.7)
            };
            let size = self.size();
            if let Some(name) = shared(self.kind.get()) {
                draw_shared(
                    snapshot,
                    name,
                    &color,
                    size as f32,
                    obj.scale_factor() as f32,
                );
            } else if size != SIZE {
                let scale = size as f32 / SIZE as f32;
                snapshot.save();
                snapshot.scale(scale, scale);
                draw(snapshot, self.kind.get(), &color);
                snapshot.restore();
            } else {
                draw(snapshot, self.kind.get(), &color);
            }
        }
    }

    impl Icon {
        fn size(&self) -> i32 {
            match self.size.get() {
                0 => SIZE,
                n => n,
            }
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

    /// 描いている形。
    pub fn kind(&self) -> Kind {
        self.imp().kind.get()
    }

    /// 16 でない大きさで描く（形は 16 のものを伸ばす。macOS 版で大きめの記号を使うところ）。
    pub fn set_pixel_size(&self, size: i32) {
        self.imp().size.set(size);
        self.queue_resize();
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

/// seal の縁（ぎざぎざの丸、12 の山）。
fn seal_outline(p: &gsk::PathBuilder) {
    for i in 0..=24 {
        let a = std::f32::consts::PI * i as f32 / 12.0;
        let r = if i % 2 == 0 { 6.6 } else { 5.5 };
        let (x, y) = (8.0 + r * a.cos(), 8.0 + r * a.sin());
        if i == 0 {
            p.move_to(x, y);
        } else {
            p.line_to(x, y);
        }
    }
    p.close();
}

/// seal の中の印（チェックか ×）。
fn seal_mark(p: &gsk::PathBuilder, cross: bool) {
    if cross {
        p.move_to(6.0, 6.0);
        p.line_to(10.0, 10.0);
        p.move_to(10.0, 6.0);
        p.line_to(6.0, 10.0);
    } else {
        p.move_to(5.4, 8.1);
        p.line_to(7.2, 9.9);
        p.line_to(10.6, 6.2);
    }
}

/// `kind` を `center` を中心に `size` px の大きさで描く（表の中など、部品でない所に描くとき）。
pub fn draw_at(
    snapshot: &gtk::Snapshot,
    kind: Kind,
    color: &gdk::RGBA,
    center: graphene::Point,
    size: f32,
) {
    snapshot.save();
    snapshot.translate(&graphene::Point::new(
        center.x() - size / 2.0,
        center.y() - size / 2.0,
    ));
    if let Some(name) = shared(kind) {
        draw_shared(snapshot, name, color, size, 1.0);
        snapshot.restore();
        return;
    }
    let scale = size / SIZE as f32;
    snapshot.scale(scale, scale);
    draw(snapshot, kind, color);
    snapshot.restore();
}

/// 共通のアイコン（`amonhen-icons`、どの FE でも同じ形）で描く種類と、その名前。まだツールバーの分だけ。
fn shared(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::Sidebar => "sidebar",
        Kind::Repositories => "repositories",
        Kind::Branch => "branch",
        Kind::NewBranch => "new-branch",
        Kind::Fetch => "fetch",
        Kind::Pull => "pull",
        Kind::Push => "push",
        Kind::Publish => "publish",
        Kind::Merge => "merge",
        Kind::Rebase => "rebase",
        Kind::StashPush => "stash",
        Kind::StashPop => "pop",
        Kind::Activity => "activity",
        Kind::Refresh => "refresh",
        Kind::Undo => "undo",
        Kind::Search => "search",
        Kind::Terminal => "terminal",
        Kind::Editor => "editor",
        _ => return None,
    })
}

/// 共通のアイコンの線の太さ（16px のとき）。Adwaita の記号のアイコンの線（2px）に合わせる。ほかの大きさでは
/// 大きさに比べて同じ割合。
const SHARED_STROKE: f32 = 2.0;

thread_local! {
    /// 作ったパス（名前・大きさ・画素の細かさごとに 1 度だけ作る）。
    #[allow(clippy::type_complexity)]
    static SHARED_PATHS: std::cell::RefCell<
        std::collections::HashMap<(&'static str, u32, u32), Vec<(gsk::Path, Vec<f32>)>>,
    > = Default::default();
}

/// 共通のアイコン `name` を、左上を原点に一辺 `size` px で線で引く。縦・横の線は画素（`scale` は画面の倍率）に
/// 合わせる（`amonhen_icons::Icon::strokes`）。
fn draw_shared(
    snapshot: &gtk::Snapshot,
    name: &'static str,
    color: &gdk::RGBA,
    size: f32,
    scale: f32,
) {
    let Some(icon) = amonhen_icons::get(name) else {
        return;
    };
    let width = SHARED_STROKE * size / SIZE as f32;
    SHARED_PATHS.with_borrow_mut(|paths| {
        let strokes = paths
            .entry((name, size.to_bits(), scale.to_bits()))
            .or_insert_with(|| {
                icon.strokes(size, scale, width)
                    .into_iter()
                    .map(|s| (path_of(&s.segments), s.dash))
                    .collect()
            });
        let stroke = gsk::Stroke::new(width);
        stroke.set_line_cap(gsk::LineCap::Round);
        stroke.set_line_join(gsk::LineJoin::Round);
        for (path, dash) in strokes.iter() {
            stroke.set_dash(dash);
            snapshot.append_stroke(path, &stroke, color);
        }
    });
}

/// `amonhen_icons` の列（移動・直線・ベジェ・閉じる）を GSK のパスに。
fn path_of(segments: &[f32]) -> gsk::Path {
    let p = gsk::PathBuilder::new();
    let mut i = 0;
    while let Some(op) = segments
        .get(i)
        .and_then(|&c| amonhen_icons::Op::from_code(c))
    {
        let c = &segments[i + 1..i + 1 + op.arity()];
        match op {
            amonhen_icons::Op::Move => p.move_to(c[0], c[1]),
            amonhen_icons::Op::Line => p.line_to(c[0], c[1]),
            amonhen_icons::Op::Cubic => p.cubic_to(c[0], c[1], c[2], c[3], c[4], c[5]),
            amonhen_icons::Op::Close => p.close(),
        }
        i += 1 + op.arity();
    }
    p.to_path()
}

/// 16×16 の中に描く。
fn draw(snapshot: &gtk::Snapshot, kind: Kind, color: &gdk::RGBA) {
    if let Some(name) = shared(kind) {
        draw_shared(snapshot, name, color, SIZE as f32, 1.0);
        return;
    }
    let p = gsk::PathBuilder::new();
    let point = |x: f32, y: f32| graphene::Point::new(x, y);
    let mut fill = None;
    // 塗りから抜く線（白抜きの印。macOS 版の .fill の記号の中の印）。
    let mut knock: Option<gsk::PathBuilder> = None;
    match kind {
        Kind::None => return,
        // 共通のアイコンで描く（上の `shared`）。
        Kind::Sidebar
        | Kind::Repositories
        | Kind::Branch
        | Kind::NewBranch
        | Kind::Fetch
        | Kind::Pull
        | Kind::Push
        | Kind::Publish
        | Kind::Merge
        | Kind::Rebase
        | Kind::StashPush
        | Kind::StashPop
        | Kind::Activity
        | Kind::Refresh
        | Kind::Undo
        | Kind::Search
        | Kind::Terminal
        | Kind::Editor => return,
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
        Kind::Subtree => {
            // 手前の四角と、右下に重なる 2 枚の縁。
            p.add_rect(&graphene::Rect::new(2.0, 2.0, 7.5, 7.5));
            for d in [2.25f32, 4.5] {
                p.move_to(2.0 + d, 9.5 + d);
                p.line_to(9.5 + d, 9.5 + d);
                p.line_to(9.5 + d, 2.0 + d);
            }
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
        Kind::FolderFill => {
            let solid = gsk::PathBuilder::new();
            solid.move_to(2.5, 4.0);
            solid.line_to(6.5, 4.0);
            solid.line_to(8.0, 5.5);
            solid.line_to(13.5, 5.5);
            solid.line_to(13.5, 12.5);
            solid.line_to(2.5, 12.5);
            solid.close();
            fill = Some(solid);
        }
        Kind::Person => {
            // 丸の中の人（頭と肩）。
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.add_circle(&point(8.0, 6.6), 2.2);
            p.move_to(3.9, 12.6);
            p.svg_arc_to(4.6, 3.6, 0.0, false, true, 12.1, 12.6);
        }
        Kind::Brush => {
            // 斜めの柄と、左下の毛先。
            p.move_to(13.5, 2.5);
            p.line_to(7.6, 8.4);
            p.move_to(7.6, 8.4);
            p.line_to(9.6, 10.4);
            p.move_to(6.6, 9.4);
            p.quad_to(4.2, 9.6, 4.0, 12.0);
            p.quad_to(3.8, 13.2, 2.6, 13.6);
            p.quad_to(6.6, 14.4, 8.4, 12.6);
            p.quad_to(9.0, 11.8, 8.6, 11.4);
            p.close();
        }
        Kind::Palette => {
            // 絵の具の板（丸と親指の穴）と、3 つの絵の具。
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.add_circle(&point(9.9, 10.7), 1.3);
            let paint = gsk::PathBuilder::new();
            for (x, y) in [(4.9_f32, 7.6_f32), (6.6, 4.6), (10.1, 4.6), (11.6, 7.6)] {
                paint.add_circle(&point(x, y), 1.05);
            }
            fill = Some(paint);
        }
        Kind::Speedometer => {
            // 下の開いた丸と、針と、目盛り。
            p.move_to(3.4, 12.6);
            p.svg_arc_to(6.3, 6.3, 0.0, true, true, 12.6, 12.6);
            p.move_to(8.0, 9.4);
            p.line_to(10.8, 5.6);
            for (x0, y0, x1, y1) in [
                (8.0_f32, 2.6_f32, 8.0_f32, 3.8_f32),
                (3.0, 7.0, 4.1, 7.4),
                (13.0, 7.0, 11.9, 7.4),
            ] {
                p.move_to(x0, y0);
                p.line_to(x1, y1);
            }
            let hub = gsk::PathBuilder::new();
            hub.add_circle(&point(8.0, 9.6), 1.1);
            fill = Some(hub);
        }
        Kind::QuestionSquare => {
            // 点線の四角（辺ごとに 2 本）と、中の「?」。
            for (x0, y0, x1, y1) in [
                (3.0_f32, 2.5_f32, 7.0_f32, 2.5_f32),
                (9.0, 2.5, 13.0, 2.5),
                (13.5, 3.0, 13.5, 7.0),
                (13.5, 9.0, 13.5, 13.0),
                (13.0, 13.5, 9.0, 13.5),
                (7.0, 13.5, 3.0, 13.5),
                (2.5, 13.0, 2.5, 9.0),
                (2.5, 7.0, 2.5, 3.0),
            ] {
                p.move_to(x0, y0);
                p.line_to(x1, y1);
            }
            p.move_to(6.4, 6.6);
            p.svg_arc_to(1.7, 1.7, 0.0, true, true, 8.5, 8.3);
            p.line_to(8.0, 9.2);
            let dot = gsk::PathBuilder::new();
            dot.add_circle(&point(8.0, 11.2), 0.75);
            fill = Some(dot);
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
        Kind::Eye => {
            // アーモンド形のまぶたと、真ん中の瞳。
            p.move_to(1.5, 8.0);
            p.quad_to(8.0, 0.5, 14.5, 8.0);
            p.quad_to(8.0, 15.5, 1.5, 8.0);
            p.close();
            p.add_circle(&point(8.0, 8.0), 2.2);
        }
        Kind::Space => {
            p.move_to(2.5, 8.5);
            p.line_to(2.5, 11.5);
            p.line_to(13.5, 11.5);
            p.line_to(13.5, 8.5);
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
        Kind::EditList => {
            // クリップボード（上に留め具）と、中の 3 本の行。右下に鉛筆。
            p.move_to(6.0, 3.0);
            p.line_to(3.5, 3.0);
            p.line_to(3.5, 14.0);
            p.line_to(8.5, 14.0);
            p.move_to(10.0, 3.0);
            p.line_to(12.5, 3.0);
            p.line_to(12.5, 7.5);
            p.add_rounded_rect(&gsk::RoundedRect::from_rect(
                graphene::Rect::new(6.0, 1.8, 4.0, 2.4),
                0.8,
            ));
            for y in [6.5, 9.0, 11.5] {
                p.move_to(5.8, y);
                p.line_to(if y > 10.0 { 7.5 } else { 10.0 }, y);
            }
            p.move_to(9.6, 14.4);
            p.line_to(9.8, 12.8);
            p.line_to(13.3, 9.3);
            p.line_to(14.7, 10.7);
            p.line_to(11.2, 14.2);
            p.close();
        }
        Kind::UpDown => {
            // 上向きの矢印（左）と下向きの矢印（右）。
            p.move_to(5.5, 13.5);
            p.line_to(5.5, 3.0);
            p.move_to(3.0, 5.5);
            p.line_to(5.5, 3.0);
            p.line_to(8.0, 5.5);
            p.move_to(10.5, 2.5);
            p.line_to(10.5, 13.0);
            p.move_to(8.0, 10.5);
            p.line_to(10.5, 13.0);
            p.line_to(13.0, 10.5);
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
        Kind::BranchDisc => {
            // 塗った丸から、Branch と同じ形を 0.75 倍にして抜く（選んだ行の地の色が透けて見える）。抜く線は太い
            // （1.6）ので、点は小さくして形が潰れないように。
            let solid = gsk::PathBuilder::new();
            solid.add_circle(&point(8.0, 8.0), 7.5);
            fill = Some(solid);
            let mark = gsk::PathBuilder::new();
            mark.add_circle(&point(5.75, 4.6), 0.5);
            mark.add_circle(&point(5.75, 11.4), 0.5);
            mark.add_circle(&point(10.25, 5.4), 0.5);
            mark.move_to(5.75, 5.9);
            mark.line_to(5.75, 10.1);
            mark.move_to(10.25, 6.7);
            mark.cubic_to(10.25, 8.8, 5.75, 8.0, 5.75, 10.1);
            knock = Some(mark);
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
        Kind::CloudAlert => {
            cloud(&p);
            p.move_to(8.0, 7.6);
            p.line_to(8.0, 9.6);
            p.move_to(8.0, 11.2);
            p.line_to(8.0, 11.3);
        }
        Kind::Scissors => {
            // 2 つの輪と、交わる 2 本の刃。
            p.add_circle(&point(4.5, 11.5), 2.0);
            p.add_circle(&point(11.5, 11.5), 2.0);
            p.move_to(5.9, 10.1);
            p.line_to(11.5, 2.5);
            p.move_to(10.1, 10.1);
            p.line_to(4.5, 2.5);
        }
        Kind::CloudDown => {
            cloud(&p);
            p.move_to(8.0, 6.8);
            p.line_to(8.0, 11.0);
            p.move_to(6.3, 9.3);
            p.line_to(8.0, 11.0);
            p.line_to(9.7, 9.3);
        }
        Kind::CircleDashed => {
            // 円を 8 つに切った弧（それぞれ短い折れ線）。
            for i in 0..8 {
                let a = std::f32::consts::PI * i as f32 / 4.0;
                for (j, t) in [0.0f32, 0.2, 0.4].into_iter().enumerate() {
                    let (x, y) = (8.0 + 6.0 * (a + t).cos(), 8.0 + 6.0 * (a + t).sin());
                    if j == 0 {
                        p.move_to(x, y);
                    } else {
                        p.line_to(x, y);
                    }
                }
            }
        }
        Kind::Pencil => {
            // 斜めの軸と、左下の先。
            p.move_to(11.0, 2.8);
            p.line_to(13.2, 5.0);
            p.line_to(5.5, 12.7);
            p.line_to(2.6, 13.4);
            p.line_to(3.3, 10.5);
            p.close();
            p.move_to(9.4, 4.4);
            p.line_to(11.6, 6.6);
        }
        Kind::FolderMinus => {
            p.move_to(2.5, 4.0);
            p.line_to(6.5, 4.0);
            p.line_to(8.0, 5.5);
            p.line_to(13.5, 5.5);
            p.line_to(13.5, 8.0);
            p.move_to(8.5, 12.5);
            p.line_to(2.5, 12.5);
            p.line_to(2.5, 4.0);
            // 右下の −。
            p.move_to(10.0, 11.0);
            p.line_to(14.5, 11.0);
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
        Kind::WrapLines => {
            // 右上から下りて左へ曲がる矢印（改行の印）。
            p.move_to(12.5, 3.0);
            p.line_to(12.5, 9.5);
            p.line_to(4.0, 9.5);
            p.move_to(7.0, 6.5);
            p.line_to(4.0, 9.5);
            p.line_to(7.0, 12.5);
        }
        Kind::ZoomOut | Kind::ZoomIn | Kind::ZoomFit => {
            // 虫めがねと、中の印（−・＋・斜めの両矢印）。
            p.add_circle(&point(6.8, 6.8), 5.0);
            p.move_to(10.4, 10.4);
            p.line_to(14.0, 14.0);
            match kind {
                Kind::ZoomFit => {
                    p.move_to(4.9, 8.7);
                    p.line_to(8.7, 4.9);
                    p.move_to(6.9, 4.9);
                    p.line_to(8.7, 4.9);
                    p.line_to(8.7, 6.7);
                    p.move_to(4.9, 6.9);
                    p.line_to(4.9, 8.7);
                    p.line_to(6.7, 8.7);
                }
                _ => {
                    p.move_to(4.4, 6.8);
                    p.line_to(9.2, 6.8);
                    if kind == Kind::ZoomIn {
                        p.move_to(6.8, 4.4);
                        p.line_to(6.8, 9.2);
                    }
                }
            }
        }
        Kind::Checkerboard => {
            // 角の丸い枠と、中の市松模様（4 × 3 の升の半分を塗る）。
            p.add_rounded_rect(&gsk::RoundedRect::from_rect(
                graphene::Rect::new(2.0, 3.0, 12.0, 10.0),
                1.5,
            ));
            let squares = gsk::PathBuilder::new();
            for row in 0..3 {
                for col in 0..4 {
                    if (row + col) % 2 == 0 {
                        squares.add_rect(&graphene::Rect::new(
                            2.0 + col as f32 * 3.0,
                            3.0 + row as f32 * 10.0 / 3.0,
                            3.0,
                            10.0 / 3.0,
                        ));
                    }
                }
            }
            fill = Some(squares);
        }
        Kind::House => {
            // 屋根と壁、戸口。
            p.move_to(2.0, 8.0);
            p.line_to(8.0, 2.5);
            p.line_to(14.0, 8.0);
            p.move_to(4.0, 6.5);
            p.line_to(4.0, 13.5);
            p.line_to(12.0, 13.5);
            p.line_to(12.0, 6.5);
            p.move_to(6.8, 13.5);
            p.line_to(6.8, 10.0);
            p.line_to(9.2, 10.0);
            p.line_to(9.2, 13.5);
        }
        Kind::CheckCircle => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(5.2, 8.2);
            p.line_to(7.2, 10.2);
            p.line_to(10.9, 6.0);
        }
        Kind::XOctagonFill => {
            // 塗った八角形と、白く抜いた ×。
            let solid = gsk::PathBuilder::new();
            for i in 0..8 {
                let a = std::f32::consts::PI * (i as f32 + 0.5) / 4.0;
                let (x, y) = (8.0 + 6.9 * a.cos(), 8.0 + 6.9 * a.sin());
                if i == 0 {
                    solid.move_to(x, y);
                } else {
                    solid.line_to(x, y);
                }
            }
            solid.close();
            let mark = gsk::PathBuilder::new();
            mark.move_to(5.8, 5.8);
            mark.line_to(10.2, 10.2);
            mark.move_to(10.2, 5.8);
            mark.line_to(5.8, 10.2);
            fill = Some(solid);
            knock = Some(mark);
        }
        Kind::StopCircle => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            let square = gsk::PathBuilder::new();
            square.add_rect(&graphene::Rect::new(5.6, 5.6, 4.8, 4.8));
            fill = Some(square);
        }
        Kind::CheckCircleFill | Kind::XCircleFill | Kind::CircleFill => {
            // 塗った円と、白く抜いた印。
            let solid = gsk::PathBuilder::new();
            solid.add_circle(&point(8.0, 8.0), 6.9);
            fill = Some(solid);
            let mark = gsk::PathBuilder::new();
            if kind == Kind::CheckCircleFill {
                mark.move_to(5.0, 8.2);
                mark.line_to(7.1, 10.3);
                mark.line_to(11.0, 5.9);
                knock = Some(mark);
            } else if kind == Kind::XCircleFill {
                mark.move_to(5.7, 5.7);
                mark.line_to(10.3, 10.3);
                mark.move_to(10.3, 5.7);
                mark.line_to(5.7, 10.3);
                knock = Some(mark);
            }
        }
        Kind::Circle => {
            p.add_circle(&point(8.0, 8.0), 6.3);
        }
        Kind::SlashCircle => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(3.5, 12.5);
            p.line_to(12.5, 3.5);
        }
        Kind::Expand => {
            // 左上と右下の角へ向かう矢印。
            p.move_to(7.0, 7.0);
            p.line_to(2.5, 2.5);
            p.move_to(2.5, 7.0);
            p.line_to(2.5, 2.5);
            p.line_to(7.0, 2.5);
            p.move_to(9.0, 9.0);
            p.line_to(13.5, 13.5);
            p.move_to(13.5, 9.0);
            p.line_to(13.5, 13.5);
            p.line_to(9.0, 13.5);
        }
        Kind::Restore => {
            // 角から真ん中へ向かう矢印。
            p.move_to(2.5, 2.5);
            p.line_to(7.0, 7.0);
            p.move_to(2.5, 7.0);
            p.line_to(7.0, 7.0);
            p.line_to(7.0, 2.5);
            p.move_to(13.5, 13.5);
            p.line_to(9.0, 9.0);
            p.move_to(13.5, 9.0);
            p.line_to(9.0, 9.0);
            p.line_to(9.0, 13.5);
        }
        Kind::BoltCircle => {
            // 円の中に、横向きの稲妻。
            p.add_circle(&point(8.0, 8.0), 6.3);
            let bolt = gsk::PathBuilder::new();
            bolt.move_to(4.2, 8.6);
            bolt.line_to(8.6, 5.0);
            bolt.line_to(8.0, 7.6);
            bolt.line_to(11.8, 7.4);
            bolt.line_to(7.4, 11.0);
            bolt.line_to(8.0, 8.4);
            bolt.close();
            fill = Some(bolt);
        }
        Kind::PencilCircle => {
            // 円の中に、小さな鉛筆。
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(5.2, 10.8);
            p.line_to(5.2, 9.4);
            p.line_to(9.4, 5.2);
            p.line_to(10.8, 6.6);
            p.line_to(6.6, 10.8);
            p.close();
        }
        Kind::PlusCircle => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(8.0, 5.0);
            p.line_to(8.0, 11.0);
            p.move_to(5.0, 8.0);
            p.line_to(11.0, 8.0);
        }
        Kind::Warning => {
            // 角を丸めない三角と、中の「!」。
            p.move_to(8.0, 2.2);
            p.line_to(14.2, 13.3);
            p.line_to(1.8, 13.3);
            p.close();
            p.move_to(8.0, 6.3);
            p.line_to(8.0, 9.6);
            p.move_to(8.0, 11.3);
            p.line_to(8.0, 11.4);
        }
        Kind::Gear => {
            // 歯車: 中の穴と、外周の 8 つの歯（それぞれ短い線）。
            p.add_circle(&point(8.0, 8.0), 2.0);
            p.add_circle(&point(8.0, 8.0), 4.6);
            for i in 0..8 {
                let a = std::f32::consts::PI * i as f32 / 4.0;
                let (c, s) = (a.cos(), a.sin());
                p.move_to(8.0 + 4.6 * c, 8.0 + 4.6 * s);
                p.line_to(8.0 + 6.6 * c, 8.0 + 6.6 * s);
            }
        }
        Kind::Seal | Kind::SealCross => {
            // 縁がぎざぎざの丸（12 の山）と、中にチェックか ×。
            seal_outline(&p);
            seal_mark(&p, kind == Kind::SealCross);
        }
        Kind::SealFill | Kind::SealCrossFill => {
            // 塗ったぎざぎざの丸と、白く抜いたチェックか ×。
            let solid = gsk::PathBuilder::new();
            seal_outline(&solid);
            let mark = gsk::PathBuilder::new();
            seal_mark(&mark, kind == Kind::SealCrossFill);
            fill = Some(solid);
            knock = Some(mark);
        }
        Kind::Shield => {
            shield(&p);
            p.move_to(5.6, 8.0);
            p.line_to(7.4, 9.8);
            p.line_to(10.4, 6.4);
        }
        Kind::ShieldAlertFill => {
            let solid = gsk::PathBuilder::new();
            shield(&solid);
            let mark = gsk::PathBuilder::new();
            mark.move_to(8.0, 5.0);
            mark.line_to(8.0, 8.6);
            mark.move_to(8.0, 11.0);
            mark.line_to(8.0, 11.1);
            fill = Some(solid);
            knock = Some(mark);
        }
        Kind::PersonQuestion => {
            // 丸の中の人（頭と肩）と、右下の小さな「?」。
            p.move_to(12.6, 6.0);
            p.svg_arc_to(6.0, 6.0, 0.0, true, false, 7.0, 13.9);
            p.add_circle(&point(7.0, 6.2), 1.9);
            p.move_to(3.4, 11.8);
            p.svg_arc_to(4.0, 3.2, 0.0, false, true, 9.6, 9.8);
            p.move_to(11.0, 9.6);
            p.svg_arc_to(1.6, 1.6, 0.0, true, true, 13.2, 11.3);
            p.line_to(12.6, 12.0);
            let dot = gsk::PathBuilder::new();
            dot.add_circle(&point(12.6, 14.2), 0.75);
            fill = Some(dot);
        }
        Kind::ClockAlert => {
            // 時計（右上を開ける）と、右の「!」。
            p.move_to(11.0, 3.1);
            p.svg_arc_to(5.6, 5.6, 0.0, true, false, 11.9, 12.2);
            p.move_to(7.4, 5.2);
            p.line_to(7.4, 8.4);
            p.line_to(9.4, 9.6);
            p.move_to(14.0, 3.0);
            p.line_to(14.0, 8.6);
            let dot = gsk::PathBuilder::new();
            dot.add_circle(&point(14.0, 11.6), 0.8);
            fill = Some(dot);
        }
        Kind::ExclamationCircle => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(8.0, 4.6);
            p.line_to(8.0, 8.8);
            let dot = gsk::PathBuilder::new();
            dot.add_circle(&point(8.0, 11.4), 0.8);
            fill = Some(dot);
        }
        Kind::Question => {
            p.add_circle(&point(8.0, 8.0), 6.3);
            p.move_to(6.0, 6.4);
            p.svg_arc_to(2.0, 2.0, 0.0, true, true, 8.6, 8.3);
            p.line_to(8.0, 9.4);
            let dot = gsk::PathBuilder::new();
            dot.add_circle(&point(8.0, 11.6), 0.8);
            fill = Some(dot);
        }
        Kind::Sparkles => {
            // 辺のへこんだ 4 つ角の星（大）と、右上の小さな星。
            for (cx, cy, r) in [(6.5_f32, 9.5_f32, 5.0_f32), (12.3, 3.7, 2.3)] {
                p.move_to(cx, cy - r);
                p.quad_to(cx, cy, cx + r, cy);
                p.quad_to(cx, cy, cx, cy + r);
                p.quad_to(cx, cy, cx - r, cy);
                p.quad_to(cx, cy, cx, cy - r);
                p.close();
            }
        }
        Kind::Bubble => {
            // 角の丸い吹き出し（左下にしっぽ）と、中の「A」。
            p.move_to(4.0, 2.5);
            p.line_to(12.0, 2.5);
            p.svg_arc_to(2.0, 2.0, 0.0, false, true, 14.0, 4.5);
            p.line_to(14.0, 9.5);
            p.svg_arc_to(2.0, 2.0, 0.0, false, true, 12.0, 11.5);
            p.line_to(6.5, 11.5);
            p.line_to(3.5, 14.0);
            p.line_to(3.8, 11.4);
            p.svg_arc_to(2.0, 2.0, 0.0, false, true, 2.0, 9.5);
            p.line_to(2.0, 4.5);
            p.svg_arc_to(2.0, 2.0, 0.0, false, true, 4.0, 2.5);
            p.close();
            p.move_to(5.6, 9.4);
            p.line_to(8.0, 4.6);
            p.line_to(10.4, 9.4);
            p.move_to(6.5, 7.7);
            p.line_to(9.5, 7.7);
        }
        Kind::ExpandVertical | Kind::CompressVertical => {
            // 真ん中の四角と、上下の矢印（開くなら外向き、畳むなら内向き）。
            p.add_rounded_rect(&gsk::RoundedRect::from_rect(
                graphene::Rect::new(3.0, 6.0, 10.0, 4.0),
                1.0,
            ));
            let (tip, base) = if kind == Kind::ExpandVertical {
                (1.0, 4.0)
            } else {
                (4.5, 1.5)
            };
            p.move_to(8.0, 1.0);
            p.line_to(8.0, 4.5);
            p.move_to(6.0, base);
            p.line_to(8.0, tip);
            p.line_to(10.0, base);
            p.move_to(8.0, 15.0);
            p.line_to(8.0, 11.5);
            p.move_to(6.0, 16.0 - base);
            p.line_to(8.0, 16.0 - tip);
            p.line_to(10.0, 16.0 - base);
        }
        Kind::ChevronLeft => {
            p.move_to(10.0, 3.0);
            p.line_to(5.0, 8.0);
            p.line_to(10.0, 13.0);
        }
        Kind::TwoUp => {
            p.add_rounded_rect(&gsk::RoundedRect::from_rect(
                graphene::Rect::new(2.0, 3.5, 12.0, 9.0),
                1.5,
            ));
            p.move_to(8.0, 3.5);
            p.line_to(8.0, 12.5);
        }
        Kind::SwipeHalf => {
            p.add_rounded_rect(&gsk::RoundedRect::from_rect(
                graphene::Rect::new(2.0, 3.5, 12.0, 9.0),
                1.5,
            ));
            let half = gsk::PathBuilder::new();
            half.add_rect(&graphene::Rect::new(8.5, 5.0, 4.0, 6.0));
            fill = Some(half);
        }
        Kind::HalfCircle => {
            p.add_circle(&point(8.0, 8.0), 6.0);
            let half = gsk::PathBuilder::new();
            half.move_to(8.0, 2.0);
            half.svg_arc_to(6.0, 6.0, 0.0, false, false, 8.0, 14.0);
            half.close();
            fill = Some(half);
        }
        Kind::PlusMinus => {
            p.move_to(2.5, 4.5);
            p.line_to(6.5, 4.5);
            p.move_to(4.5, 2.5);
            p.line_to(4.5, 6.5);
            p.move_to(12.0, 2.5);
            p.line_to(4.0, 13.5);
            p.move_to(9.5, 11.5);
            p.line_to(13.5, 11.5);
        }
    }
    let stroke = gsk::Stroke::new(1.3);
    stroke.set_line_cap(gsk::LineCap::Round);
    stroke.set_line_join(gsk::LineJoin::Round);
    snapshot.append_stroke(&p.to_path(), &stroke, color);
    match (fill, knock) {
        (Some(f), Some(k)) => {
            // 先に抜く線を印（mask）にして、塗りをその外だけに描く。
            snapshot.push_mask(gsk::MaskMode::InvertedAlpha);
            let wide = gsk::Stroke::new(1.6);
            wide.set_line_cap(gsk::LineCap::Round);
            wide.set_line_join(gsk::LineJoin::Round);
            snapshot.append_stroke(&k.to_path(), &wide, color);
            snapshot.pop();
            snapshot.append_fill(&f.to_path(), gsk::FillRule::Winding, color);
            snapshot.pop();
        }
        (Some(f), None) => snapshot.append_fill(&f.to_path(), gsk::FillRule::Winding, color),
        _ => {}
    }
}

/// 盾の形（上が平ら、下がとがる）。
fn shield(p: &gsk::PathBuilder) {
    p.move_to(8.0, 1.8);
    p.line_to(13.2, 3.8);
    p.line_to(13.2, 7.6);
    p.quad_to(13.2, 12.2, 8.0, 14.4);
    p.quad_to(2.8, 12.2, 2.8, 7.6);
    p.line_to(2.8, 3.8);
    p.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 共通のアイコンで描く種類は、どれも `amonhen-icons` にある（ないと何も描かれない）。
    #[test]
    fn shared_icons_exist() {
        for kind in [
            Kind::Sidebar,
            Kind::Repositories,
            Kind::Branch,
            Kind::NewBranch,
            Kind::Fetch,
            Kind::Pull,
            Kind::Push,
            Kind::Publish,
            Kind::Merge,
            Kind::Rebase,
            Kind::StashPush,
            Kind::StashPop,
            Kind::Activity,
            Kind::Refresh,
            Kind::Undo,
            Kind::Search,
            Kind::Terminal,
            Kind::Editor,
        ] {
            let name = shared(kind).unwrap();
            assert!(amonhen_icons::get(name).is_some(), "{name}");
        }
    }
}
