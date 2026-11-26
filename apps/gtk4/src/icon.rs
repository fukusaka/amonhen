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
    /// サイドバーの Pull request の行
    PullRequest,
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
    /// 変更のない worktree をまとめた行（共通のアイコンの clean）
    Clean,
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
            } else {
                // 文字の色のまま（Adwaita の記号のアイコンと同じ濃さ）。
                fg
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
    }
    snapshot.restore();
}

/// 種類ごとの共通のアイコン（`amonhen-icons`、どの FE でも同じ形）の名前。`None` は描かない。
fn shared(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::None => return None,
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
        Kind::PullRequest => "pull-request",
        Kind::StashPush => "stash",
        Kind::StashPop => "pop",
        Kind::Activity => "activity",
        Kind::Refresh => "refresh",
        Kind::Undo => "undo",
        Kind::Search => "search",
        Kind::Terminal => "terminal",
        Kind::Editor => "editor",
        Kind::WorkingCopy => "working-copy",
        Kind::History => "history",
        Kind::Reflog => "reflog",
        Kind::Remote => "remote",
        Kind::Tag => "tag",
        Kind::Stash => "stashes",
        Kind::Submodule => "submodule",
        Kind::Subtree => "subtree",
        Kind::Folder => "folder",
        Kind::FolderFill => "folder",
        Kind::Filter => "filter",
        Kind::List => "list",
        Kind::Tree => "tree",
        Kind::Doc => "files",
        Kind::RangeDiff => "range-diff",
        Kind::UpDown => "ahead-behind",
        Kind::Person => "account",
        Kind::Brush => "appearance",
        Kind::Palette => "theme",
        Kind::Speedometer => "performance",
        Kind::QuestionSquare => "unknown-file",
        Kind::Sliders => "display-options",
        Kind::Location => "go-to-head",
        Kind::BranchDisc => "current-branch",
        Kind::Star => "favorite",
        Kind::Stacked => "stacked",
        Kind::CloudSlash => "upstream-gone",
        Kind::CloudAlert => "remote-conflict",
        Kind::House => "main-worktree",
        Kind::CheckCircle => "ci-passed",
        Kind::Clean => "clean",
        Kind::Gear => "settings",
        Kind::Warning => "conflict",
        Kind::PlusCircle => "new-branch-row",
        Kind::PencilCircle => "edited-worktree",
        Kind::Seal => "signed",
        Kind::SealCross => "bad-signature",
        Kind::Question => "missing-key",
        Kind::SealFill => "signed",
        Kind::SealCrossFill => "bad-signature",
        Kind::Shield => "trust-unknown",
        Kind::ShieldAlertFill => "revoked-key",
        Kind::PersonQuestion => "signer-mismatch",
        Kind::ClockAlert => "expired-key",
        Kind::ExclamationCircle => "cannot-verify",
        Kind::Sparkles => "ai",
        Kind::Bubble => "translate",
        Kind::Scissors => "shallow",
        Kind::CloudDown => "partial",
        Kind::FolderMinus => "sparse",
        Kind::CircleDashed => "not-checked-out",
        Kind::Pencil => "modified",
        Kind::EditList => "edit-mode",
        Kind::XOctagonFill => "failed",
        Kind::StopCircle => "cancelled",
        Kind::BoltCircle => "disconnected",
        Kind::CheckCircleFill => "ci-passed",
        Kind::XCircleFill => "ci-failed",
        Kind::CircleFill => "ci-running",
        Kind::Circle => "ci-none",
        Kind::SlashCircle => "ci-cancelled",
        Kind::Expand => "expand",
        Kind::Restore => "restore",
        Kind::ExpandVertical => "expand-all",
        Kind::CompressVertical => "collapse-all",
        Kind::ChevronLeft => "back",
        Kind::TwoUp => "side-by-side",
        Kind::SwipeHalf => "swipe",
        Kind::HalfCircle => "onion-skin",
        Kind::PlusMinus => "changed-pixels",
        Kind::Space => "whitespace",
        Kind::Eye => "preview",
        Kind::ZoomOut => "zoom-out",
        Kind::ZoomIn => "zoom-in",
        Kind::ZoomFit => "zoom-fit",
        Kind::Checkerboard => "checkerboard",
        Kind::WrapLines => "wrap-lines",
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

#[cfg(test)]
mod tests {
    /// どの種類の名前も `amonhen-icons` にある（ないと何も描かれない）。
    #[test]
    fn shared_icons_exist() {
        let src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/icon.rs")).unwrap();
        let mut checked = 0;
        for line in src.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Kind::")
                && let Some((_, name)) = rest.split_once(" => \"")
                && let Some(name) = name.strip_suffix("\",")
            {
                assert!(amonhen_icons::get(name).is_some(), "{name}");
                checked += 1;
            }
        }
        assert!(checked > 90, "{checked}");
    }
}
