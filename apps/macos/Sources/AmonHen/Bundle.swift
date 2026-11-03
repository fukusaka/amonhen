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
        let save = alert.addButton(withTitle: L("Save…"))
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
        // 「後だけ」を選んで基点が空なら、Save… は押せない（黙って全体を保存しない）。
        let updateSave = ClosureTarget {
            save.isEnabled = newer.state != .on || !field.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
        }
        let toggle = ClosureTarget { [weak alert] in
            field.isEnabled = newer.state == .on
            if field.isEnabled { alert?.window.makeFirstResponder(field) }
            updateSave.fire()
        }
        let typing = NotificationCenter.default.addObserver(
            forName: NSControl.textDidChangeNotification, object: field, queue: .main
        ) { _ in MainActor.assumeIsolated { updateSave.fire() } }
        defer { NotificationCenter.default.removeObserver(typing) }
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

/// `bundleCheck` の答え。
nonisolated private struct BundleCheck: Decodable, Sendable {
    let refs: [String]
    let missing: Int
    let name: String
}

extension Actions {
    /// Repository > Fetch from Bundle…: bundle のファイルを選び、そのブランチを `refs/remotes/<名前>/*` に取り込む
    /// （タグは取らない）。要るコミットがなければ、そう言って止める。SSH 先のリポジトリにも、中身を送って取り込む。
    func fetchFromBundle() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "bundle") ?? .data]
        panel.canChooseDirectories = false
        panel.message = L("Choose a bundle file (git bundle) to fetch from.")
        guard panel.runModal() == .OK, let file = panel.url?.path else { return }
        let repository = self.repository
        Task {
            let command: [String: Any] = ["op": "bundleCheck", "file": file]
            let json: String
            do { json = try Repository.json(command) } catch { return }
            let checked = await Task.detached { Result { try repository.info(json: json, as: BundleCheck.self) } }.value
            let check: BundleCheck
            switch checked {
            case let .success(c): check = c
            case let .failure(error):
                await fail(OperationFailure(error, command: command))
                return
            }
            if check.missing > 0 {
                await inform(L("Can’t Fetch the Bundle"),
                             L("This repository lacks %ld commits the bundle needs (it has only the commits after them).", check.missing))
                return
            }
            let branches = check.refs.filter { $0.hasPrefix("refs/heads/") }.map { String($0.dropFirst(11)) }
            if branches.isEmpty {
                await inform(L("Nothing to Fetch"), L("The bundle has no branches."))
                return
            }
            guard let name = await askBundleName(file: file, branches: branches, suggested: check.name) else { return }
            perform(["op": "fetchBundle", "file": file, "name": name])
        }
    }

    /// 取り込む名前を尋ねる。キャンセルなら nil。
    @MainActor
    private func askBundleName(file: String, branches: [String], suggested: String) async -> String? {
        let alert = NSAlert()
        alert.messageText = L("Fetch from Bundle")
        let shown = branches.prefix(5).joined(separator: ", ") + (branches.count > 5 ? ", …" : "")
        alert.informativeText = L("%@ has %ld branches (%@). They are fetched under the name below, like a remote’s branches; tags aren’t fetched.",
                                  (file as NSString).lastPathComponent, branches.count, shown)
        alert.addButton(withTitle: L("Fetch"))
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(string: suggested)
        field.widthAnchor.constraint(equalToConstant: 220).isActive = true
        let row = NSStackView(views: [NSTextField(labelWithString: L("Name:")), field])
        row.frame = NSRect(x: 0, y: 0, width: 300, height: 24)
        alert.accessoryView = row
        alert.window.initialFirstResponder = field
        guard await present(alert) == .alertFirstButtonReturn else { return nil }
        let name = field.stringValue.trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? nil : name
    }
}
