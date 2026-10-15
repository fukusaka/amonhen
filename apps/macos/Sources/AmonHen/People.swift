import AmonhenKit
import AppKit

/// コミットの詳細の上に、作者とコミッターを左右に並べる帯（アバター・名前・メール・日時）。
final class PeopleStrip: NSView {
    static let height: CGFloat = 48

    /// 名前を押したとき（History をその人のコミットで絞り込む）。
    var onSelectPerson: ((String) -> Void)?

    private let author = PersonView(role: "Author")
    private let committer = PersonView(role: "Committer")

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
        author.show(name: info.author, email: info.authorEmail, date: "\(date(info.authorTime)) (\(info.authorTz))")
        committer.show(name: info.committer, email: info.committerEmail, date: "\(date(info.committerTime)) (\(info.committerTz))")
        // 作者とコミッターが同じ人（名前とメールが同じ）なら、作者だけを出す。
        committer.isHidden = info.committer == info.author
            && info.committerEmail.lowercased() == info.authorEmail.lowercased()
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
        name.toolTip = "この人のコミットで絞り込む"
        self.role.font = .systemFont(ofSize: 10)
        self.role.textColor = .tertiaryLabelColor
        detail.font = .systemFont(ofSize: 11)
        detail.textColor = .secondaryLabelColor
        detail.isSelectable = true
        detail.maximumNumberOfLines = 2
        detail.lineBreakMode = .byTruncatingTail
        for v in [avatar, name, self.role, detail] as [NSView] { addSubview(v) }
    }

    required init?(coder: NSCoder) { fatalError() }

    override func layout() {
        super.layout()
        let size: CGFloat = 32, x: CGFloat = 8
        avatar.frame = NSRect(x: x, y: (bounds.height - size) / 2, width: size, height: size)
        let textX = x + size + 8
        name.sizeToFit()
        name.frame.origin = NSPoint(x: textX - 2, y: bounds.height - name.frame.height - 3)
        role.sizeToFit()
        role.frame.origin = NSPoint(x: name.frame.maxX + 4, y: name.frame.minY + 2)
        detail.frame = NSRect(x: textX, y: 2, width: max(0, bounds.width - textX - 6), height: name.frame.minY - 2)
    }

    func show(name: String, email: String, date: String) {
        person = (name, email)
        self.name.title = name
        detail.stringValue = "\(email)\n\(date)"
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
