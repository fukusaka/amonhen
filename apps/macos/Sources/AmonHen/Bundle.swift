import AmonhenKit
import AppKit
import UniformTypeIdentifiers

/// `bundlePreview` の答え。
nonisolated private struct BundlePreview: Decodable, Sendable {
    let commits: Int
    let contents: Int
    let more: Bool
    let beyondShallow: Bool
}

/// bundle: コミットを、git が clone や fetch できる 1 つのファイルにまとめる（git bundle create）。ファイルはいつも
/// 手元に書く（SSH 先のリポジトリでも、コアが中身をやりとりする）。
extension Actions {
    /// `refs`（`refs/heads/main` など。空なら HEAD とすべてのブランチ・タグ）を bundle に書く。`names` は見せる名前、
    /// `label` は既定のファイル名に使う。入れる範囲（全体か、ある基点より後か）を選んでもらい、入るコミットの数と
    /// partial clone で取ってくる中身を確かめてから、保存する場所を選んでもらう。書いたら Finder で見せる。
    func saveBundle(refs: [String], names: [String], label: String) {
        let repository = self.repository
        let base = (repository.workdir.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "")
        let name = Self.archiveName(base.isEmpty ? label : "\(base)-\(label)")
        Task {
            guard let after = await askBundleRange(names: names) else { return }
            let bases = after.map { [$0] } ?? []
            let command: [String: Any] = ["op": "bundlePreview", "refs": refs, "bases": bases]
            let json: String
            do { json = try Repository.json(command) } catch { return }
            let preview = await Task.detached { Result { try repository.info(json: json, as: BundlePreview.self) } }.value
            let shown: BundlePreview
            switch preview {
            case let .success(p): shown = p
            case let .failure(error):
                await fail(OperationFailure(error, command: command))
                return
            }
            if shown.commits == 0 {
                await inform(L("Nothing to Save"), L("There are no commits to put in the bundle."))
                return
            }
            if shown.beyondShallow {
                await inform(L("Can’t Save the Bundle"),
                             L("This shallow clone doesn’t have all the history the bundle needs. Fetch more history, or choose a newer base."))
                return
            }
            if shown.contents > 0 {
                let alert = NSAlert()
                alert.messageText = L("Fetch File Contents First?")
                alert.informativeText = L("This is a partial clone: the contents of %@ files in the bundle aren’t here yet. They are fetched from the remote in one go before the bundle is written.",
                                          shown.more ? "\(shown.contents)+" : "\(shown.contents)")
                alert.addButton(withTitle: L("Fetch and Save"))
                alert.addButton(withTitle: L("Cancel"))
                guard await present(alert) == .alertFirstButtonReturn else { return }
            }
            let panel = NSSavePanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "bundle") ?? .data]
            panel.nameFieldStringValue = name + ".bundle"
            panel.message = L("%ld commits", shown.commits)
            panel.canCreateDirectories = true
            guard panel.runModal() == .OK, let url = panel.url else { return }
            let save: [String: Any] = ["op": "bundle", "refs": refs, "bases": bases, "file": url.path]
            do {
                let json = try Repository.json(save)
                RunningOperations.begin()
                let result = await Task.detached(priority: .userInitiated) {
                    Result { try repository.run(json: json) }
                }.value
                RunningOperations.end()
                try result.get()
                NSWorkspace.shared.activateFileViewerSelecting([url])
            } catch {
                await fail(OperationFailure(error, command: save))
            }
        }
    }

    /// 入れる範囲を選んでもらう。全体なら nil、ある基点より後だけならその基点（`.some`）。キャンセルなら nil の nil。
    @MainActor
    private func askBundleRange(names: [String]) async -> String?? {
        let alert = NSAlert()
        alert.messageText = L("Save as Bundle")
        let what = names.isEmpty ? L("HEAD and all branches and tags") : names.joined(separator: ", ")
        alert.informativeText = L("%@, in one file that git can clone or fetch from (git bundle).", what)
        alert.addButton(withTitle: L("Save…"))
        alert.addButton(withTitle: L("Cancel"))
        let everything = NSButton(radioButtonWithTitle: L("All history"), target: nil, action: nil)
        let newer = NSButton(radioButtonWithTitle: L("Only commits after:"), target: nil, action: nil)
        everything.state = .on
        let field = NSTextField(string: "")
        field.placeholderString = L("Tag, branch or commit")
        field.widthAnchor.constraint(equalToConstant: 200).isActive = true
        field.isEnabled = false
        let note = NSTextField(wrappingLabelWithString: L("Whoever receives the bundle needs that commit already."))
        note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        note.textColor = .secondaryLabelColor
        let toggle = ClosureTarget { [weak alert] in
            field.isEnabled = newer.state == .on
            if field.isEnabled { alert?.window.makeFirstResponder(field) }
        }
        for b in [everything, newer] {
            b.target = toggle
            b.action = #selector(ClosureTarget.fire)
        }
        let stack = NSStackView(views: [everything, NSStackView(views: [newer, field]), note])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: 380, height: 84)
        alert.accessoryView = stack
        let response = await present(alert)
        _ = toggle
        guard response == .alertFirstButtonReturn else { return nil }
        let base = field.stringValue.trimmingCharacters(in: .whitespaces)
        return newer.state == .on && !base.isEmpty ? .some(base) : .some(nil)
    }
}
