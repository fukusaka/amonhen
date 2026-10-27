import AmonhenKit
import AppKit

/// コミットの詳細の上に、作者とコミッターを左右に並べる帯（アバター・名前・メール・日時）。名前とメールは mailmap で
/// 置き換えたもので、記録と違えば記録されたものを下に小さく添える。
final class PeopleStrip: NSView {
    static let height: CGFloat = 48
    /// 記録されたものを添える行の高さ。
    static let recordedHeight: CGFloat = 14

    /// 今の人に合う高さ（記録されたものを添えるなら、その分だけ高い）。
    private(set) var preferredHeight = PeopleStrip.height

    /// 名前を押したとき（History をその人のコミットで絞り込む）。
    var onSelectPerson: ((String) -> Void)?

    private let author = PersonView(role: L("Author"))
    private let committer = PersonView(role: L("Committer"))

    override init(frame: NSRect) {
        super.init(frame: frame)
        for v in [author, committer] {
            addSubview(v)
            v.onSelect = { [weak self] name in self?.onSelectPerson?(name) }
        }
    }

    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        // コミッターを出さないときは、作者だけで幅を使う。
        let half = committer.isHidden ? bounds.width : (bounds.width / 2).rounded()
        author.frame = NSRect(x: 0, y: 0, width: half, height: bounds.height)
        committer.frame = NSRect(x: half, y: 0, width: bounds.width - half, height: bounds.height)
    }

    func show(_ info: CommitInfo, date: (Int64) -> String) {
        author.show(name: info.author, email: info.authorEmail, date: "\(date(info.authorTime)) (\(info.authorTz))",
                    recorded: info.recordedAuthor)
        committer.show(name: info.committer, email: info.committerEmail, date: "\(date(info.committerTime)) (\(info.committerTz))",
                       recorded: info.recordedCommitter)
        // 作者とコミッターが同じ人（名前とメールが同じ、記録も同じ）なら、作者だけを出す。
        committer.isHidden = info.committer == info.author
            && info.committerEmail.lowercased() == info.authorEmail.lowercased()
            && info.recordedCommitter == info.recordedAuthor
        let recorded = info.recordedAuthor != nil || (!committer.isHidden && info.recordedCommitter != nil)
        preferredHeight = Self.height + (recorded ? Self.recordedHeight : 0)
        needsLayout = true
    }
}

/// 1 人分: 左にアバター、右に名前（と役割）、メール、日時。
private final class PersonView: NSView {
    var onSelect: ((String) -> Void)?
    private let avatar = NSImageView()
    private let name = NSButton()
    private let role: NSTextField
    private let detail = NSTextField(labelWithString: "")
    /// 記録された名前とメール（mailmap で置き換えて違うときだけ）。
    private let recorded = NSTextField(labelWithString: "")
    private var person = ("", "")

    init(role: String) {
        self.role = NSTextField(labelWithString: role)
        super.init(frame: .zero)
        avatar.imageScaling = .scaleProportionallyUpOrDown
        name.isBordered = false
        name.font = .systemFont(ofSize: 12, weight: .semibold)
        name.contentTintColor = .labelColor
        name.target = self
        name.action = #selector(selectPerson)
        name.toolTip = L("Filter to this person’s commits")
        self.role.font = .systemFont(ofSize: 10)
        self.role.textColor = .tertiaryLabelColor
        detail.font = .systemFont(ofSize: 11)
        detail.textColor = .secondaryLabelColor
        detail.isSelectable = true
        detail.maximumNumberOfLines = 2
        detail.lineBreakMode = .byTruncatingTail
        recorded.font = .systemFont(ofSize: 10)
        recorded.textColor = .tertiaryLabelColor
        recorded.isSelectable = true
        recorded.lineBreakMode = .byTruncatingTail
        recorded.toolTip = L("The name and email recorded in the commit. The mailmap shows them as above.")
        for v in [avatar, name, self.role, detail, recorded] as [NSView] { addSubview(v) }
    }

    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        let size: CGFloat = 32, x: CGFloat = 8
        // 記録されたものを添える行は、帯の下に足した分に置く（上の 3 行の位置は変えない）。
        let bottom: CGFloat = bounds.height > PeopleStrip.height ? bounds.height - PeopleStrip.height : 0
        avatar.frame = NSRect(x: x, y: bottom + (PeopleStrip.height - size) / 2, width: size, height: size)
        let textX = x + size + 8
        name.sizeToFit()
        name.frame.origin = NSPoint(x: textX - 2, y: bounds.height - name.frame.height - 3)
        role.sizeToFit()
        role.frame.origin = NSPoint(x: name.frame.maxX + 4, y: name.frame.minY + 2)
        detail.frame = NSRect(x: textX, y: bottom + 2, width: max(0, bounds.width - textX - 6), height: name.frame.minY - bottom - 2)
        recorded.frame = NSRect(x: textX, y: 1, width: max(0, bounds.width - textX - 6), height: max(0, bottom - 1))
    }

    func show(name: String, email: String, date: String, recorded identity: CommitInfo.Identity? = nil) {
        person = (name, email)
        self.name.title = name
        detail.stringValue = "\(email)\n\(date)"
        recorded.stringValue = identity.map { L("Recorded: %@", "\($0.name) <\($0.email)>") } ?? ""
        recorded.isHidden = identity == nil
        updateAvatar()
        needsLayout = true
    }

    /// アバターを描き直す（読み終えたとき）。
    func updateAvatar() {
        avatar.image = Settings.showAvatars ? Avatars.shared.circle(for: person.1, name: person.0, size: 32) : nil
        avatar.isHidden = !Settings.showAvatars
    }

    @objc private func selectPerson() { onSelect?(person.0) }
}

extension PeopleStrip {
    /// アバターを描き直す（読み終えたときや、設定を変えたとき）。
    func updateAvatars() {
        for v in subviews { (v as? PersonView)?.updateAvatar() }
    }
}
