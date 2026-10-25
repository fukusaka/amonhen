import AmonhenKit
import AppKit
import SwiftUI

/// リモートの追加・編集のシート（docs/ux.md の 4.5 のリモート）: 名前、fetch の URL、push の URL（空なら fetch と
/// 同じ）、URL の先が何か（GitHub・GitLab・パス・SSH 先）、保存する前の接続の確認（コアの `checkRemote`。SSH 先の
/// リポジトリなら SSH 先から）。編集なら、ここから消すこともできる。
enum RemoteSheet {
    /// `remote` が nil なら新しいリモート。`perform` が書く命令（`changeRemote` の列）を実行する。
    static func present(in window: NSWindow, repository: Repository, remote: RemoteEntry?, perform: @escaping ([[String: Any]]) -> Void) {
        let model = RemoteSheetModel(repository: repository, remote: remote, perform: perform)
        let host = NSHostingController(rootView: RemoteSheetView(model: model))
        let sheet = NSWindow(contentViewController: host)
        sheet.styleMask = [.titled]
        model.close = { [weak window, weak sheet] in
            guard let window, let sheet else { return }
            window.endSheet(sheet)
        }
        window.beginSheet(sheet)
    }

    /// リモートを消す前の確認。設定のページ、サイドバー、シートのどこから消しても同じ文言と同じボタン。
    static func confirmRemove(_ name: String) -> Bool {
        let alert = NSAlert()
        alert.messageText = L("Remove %@?", name)
        alert.informativeText = L("This removes the remote’s settings and its remote-tracking branches (%@/…). The remote repository isn’t touched.", name)
        alert.addButton(withTitle: L("Delete"))
        alert.addButton(withTitle: L("Cancel"))
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// URL の先が何か: GitHub・GitLab などのサービス、パス、SSH 先、それ以外はホスト名。
    static func kind(of url: String) -> String {
        let lower = url.lowercased()
        for (host, name) in [("github.com", "GitHub"), ("gitlab", "GitLab"), ("bitbucket", "Bitbucket"), ("codeberg", "Codeberg")]
        where lower.contains(host) {
            return name
        }
        if lower.hasPrefix("/") || lower.hasPrefix("./") || lower.hasPrefix("../") || lower.hasPrefix("~") || lower.hasPrefix("file://") {
            return L("A path")
        }
        if lower.hasPrefix("ssh://") || lower.hasPrefix("git+ssh://"), let host = URL(string: url)?.host {
            return L("SSH host %@", host)
        }
        // scp の形（user@host:path）。
        if !lower.contains("://"), let colon = url.firstIndex(of: ":") {
            let left = url[..<colon]
            let host = left.split(separator: "@").last.map(String.init) ?? String(left)
            return L("SSH host %@", host)
        }
        if let host = URL(string: url)?.host { return host }
        return url.isEmpty ? "" : L("Other")
    }
}

@Observable
final class RemoteSheetModel {
    let repository: Repository
    /// 編集しているリモート（新しいリモートなら nil）。
    let remote: RemoteEntry?
    let perform: ([[String: Any]]) -> Void
    var close: () -> Void = {}
    var name: String
    var fetchUrl: String
    var pushUrl: String
    var checking = false
    /// 接続の確認の結果（つながったときの要約か、エラー）。
    var checkResult: (text: String, ok: Bool)?
    var error: String?

    init(repository: Repository, remote: RemoteEntry?, perform: @escaping ([[String: Any]]) -> Void) {
        self.repository = repository
        self.remote = remote
        self.perform = perform
        name = remote?.name ?? "origin"
        fetchUrl = remote?.fetchUrl ?? ""
        pushUrl = remote.map { $0.pushUrl == $0.fetchUrl ? "" : $0.pushUrl } ?? ""
    }

    var canSave: Bool {
        !name.trimmingCharacters(in: .whitespaces).isEmpty && !fetchUrl.trimmingCharacters(in: .whitespaces).isEmpty && !checking
    }

    /// `git ls-remote` でつないでみる（裏で。結果はシートに出す）。
    func check() {
        let url = fetchUrl.trimmingCharacters(in: .whitespaces)
        guard !url.isEmpty else { return }
        checking = true
        checkResult = nil
        let repository = self.repository
        Task {
            nonisolated struct Check: Decodable { let branches: Int; let tags: Int; let head: String? }
            do {
                let c = try await Task.detached(priority: .userInitiated) {
                    try repository.info(["op": "checkRemote", "url": url], as: Check.self)
                }.value
                var text = L("Reachable: %ld branches, %ld tags", c.branches, c.tags)
                if let head = c.head { text += L(", HEAD at %@", head) }
                checkResult = (text, true)
            } catch {
                checkResult = (error.localizedDescription, false)
            }
            checking = false
        }
    }

    /// 書く命令を組んで実行し、閉じる。
    func save() {
        let name = self.name.trimmingCharacters(in: .whitespaces)
        let fetch = fetchUrl.trimmingCharacters(in: .whitespaces)
        let push = pushUrl.trimmingCharacters(in: .whitespaces)
        var changes: [[String: Any]] = []
        if let remote {
            if remote.name != name { changes.append(["kind": "rename", "from": remote.name, "to": name]) }
            if remote.fetchUrl != fetch { changes.append(["kind": "setUrl", "name": name, "url": fetch]) }
            let hadPush = remote.pushUrl != remote.fetchUrl ? remote.pushUrl : ""
            if hadPush != push { changes.append(["kind": "setPushUrl", "name": name, "url": push.isEmpty ? NSNull() : push]) }
        } else {
            changes.append(["kind": "add", "name": name, "url": fetch])
            if !push.isEmpty { changes.append(["kind": "setPushUrl", "name": name, "url": push]) }
        }
        perform(changes.map { ["op": "changeRemote", "change": $0] })
        close()
    }

    func remove() {
        guard let remote, RemoteSheet.confirmRemove(remote.name) else { return }
        perform([["op": "changeRemote", "change": ["kind": "remove", "name": remote.name]]])
        close()
    }
}

private struct RemoteSheetView: View {
    @Bindable var model: RemoteSheetModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(model.remote == nil ? L("Add Remote") : L("Edit Remote")).font(.headline)
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 8) {
                GridRow {
                    Text(L("Name")).gridColumnAlignment(.trailing)
                    TextField("", text: $model.name).labelsHidden()
                }
                GridRow {
                    Text(L("Fetch URL"))
                    TextField("", text: $model.fetchUrl).labelsHidden()
                }
                GridRow {
                    Text(L("Push URL"))
                    TextField("", text: $model.pushUrl, prompt: Text(L("Same as the fetch URL"))).labelsHidden()
                }
                GridRow {
                    Text(L("Points to"))
                    Text(RemoteSheet.kind(of: model.fetchUrl)).foregroundStyle(.secondary)
                }
            }
            HStack(spacing: 8) {
                Button(L("Check Connection")) { model.check() }
                    .disabled(model.fetchUrl.trimmingCharacters(in: .whitespaces).isEmpty || model.checking)
                if model.checking {
                    ProgressView().controlSize(.small)
                    Text(L("Connecting…")).foregroundStyle(.secondary)
                } else if let r = model.checkResult {
                    Text(r.text).foregroundStyle(r.ok ? Color.green : Color.red).lineLimit(3)
                }
                Spacer()
            }
            if let error = model.error {
                Text(error).foregroundStyle(.red)
            }
            HStack {
                if model.remote != nil {
                    Button(L("Remove…"), role: .destructive) { model.remove() }
                }
                Spacer()
                Button(L("Cancel")) { model.close() }.keyboardShortcut(.cancelAction)
                Button(L("Save")) { model.save() }.keyboardShortcut(.defaultAction).disabled(!model.canSave)
            }
        }
        .padding(20)
        .frame(width: 480)
    }
}
