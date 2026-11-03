import AmonhenKit
import AppKit
import UniformTypeIdentifiers

/// アーカイブ: ある時点のファイルを zip・tar.gz・tar に書き出す（git archive）。ファイルはいつも手元に書く（SSH 先の
/// リポジトリでも、コアが中身をやりとりする）。何を外すかは git の `.gitattributes`（`export-ignore`）に任せる。
extension Actions {
    /// 書き出せる形（コアの `archive` の `format`）と拡張子。
    nonisolated static let archiveFormats: [(format: String, title: String, ext: String)] = [
        ("zip", "zip", "zip"), ("tar.gz", "tar.gz", "tar.gz"), ("tar", "tar", "tar"),
    ]

    /// `rev` の時点を書き出す。`label` は既定の名前に使う（タグ・ブランチの名前、なければ短い ID）。形・先頭の
    /// フォルダ・書き出すフォルダを選んでもらい、partial clone で取ってくる中身があれば確かめてから、保存する場所を
    /// 選んでもらう。書いたら Finder で見せる。
    func exportArchive(rev: String, label: String) {
        let repository = self.repository
        let base = (repository.workdir.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "")
        let name = Self.archiveName(base.isEmpty ? label : "\(base)-\(label)")
        Task {
            guard let options = await askArchiveOptions(rev: rev, name: name) else { return }
            var paths: [String] = []
            if let folder = options.folder { paths = [folder] }
            struct Need: Decodable { let count: Int; let more: Bool }
            let need = await Task.detached {
                try? repository.info(["op": "archiveContents", "rev": rev, "paths": paths], as: Need.self)
            }.value
            if let need, need.count > 0 {
                let alert = NSAlert()
                alert.messageText = L("Fetch File Contents First?")
                alert.informativeText = L("This is a partial clone: the contents of %@ files in the archive aren’t here yet. They are fetched from the remote in one go before the archive is written.",
                                          need.more ? "\(need.count)+" : "\(need.count)")
                alert.addButton(withTitle: L("Fetch and Export"))
                alert.addButton(withTitle: L("Cancel"))
                guard await present(alert) == .alertFirstButtonReturn else { return }
            }
            let ext = Self.archiveFormats.first { $0.format == options.format }?.ext ?? options.format
            let panel = NSSavePanel()
            panel.allowedContentTypes = [UTType(filenameExtension: ext) ?? .data]
            panel.nameFieldStringValue = (options.prefix.isEmpty ? name : options.prefix) + ".\(ext)"
            panel.canCreateDirectories = true
            guard panel.runModal() == .OK, let url = panel.url else { return }
            let command: [String: Any] = [
                "op": "archive", "rev": rev, "format": options.format,
                "prefix": options.prefix.isEmpty ? "" : options.prefix + "/", "paths": paths, "file": url.path,
            ]
            do {
                let json = try Repository.json(command)
                RunningOperations.begin()
                let result = await Task.detached(priority: .userInitiated) {
                    Result { try repository.run(json: json) }
                }.value
                RunningOperations.end()
                try result.get()
                NSWorkspace.shared.activateFileViewerSelecting([url])
            } catch {
                await fail(OperationFailure(error, command: command))
            }
        }
    }

    private struct ArchiveOptions {
        var format: String
        /// 中身の先頭に置くフォルダ（空なら置かない）。
        var prefix: String
        /// 書き出すフォルダ（nil なら全体）。
        var folder: String?
    }

    /// 書き出し方を選んでもらう。形は前の選び方を覚えておく。キャンセルなら nil。
    @MainActor
    private func askArchiveOptions(rev: String, name: String) async -> ArchiveOptions? {
        let defaults = UserDefaults.standard
        let alert = NSAlert()
        alert.messageText = L("Export Archive")
        alert.informativeText = L("The files of this commit, as git archive writes them; .gitattributes decides what is left out (export-ignore).")
        alert.addButton(withTitle: L("Export…"))
        alert.addButton(withTitle: L("Cancel"))
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        format.addItems(withTitles: Self.archiveFormats.map(\.title))
        let remembered = defaults.string(forKey: "archiveFormat") ?? "zip"
        format.selectItem(at: Self.archiveFormats.firstIndex { $0.format == remembered } ?? 0)
        let top = NSTextField(string: name)
        top.placeholderString = L("None")
        top.widthAnchor.constraint(equalToConstant: 220).isActive = true
        let folder = NSTextField(labelWithString: L("All files"))
        var chosen: String?
        let choose = NSButton(title: L("Choose…"), target: nil, action: nil)
        let all = NSButton(title: L("All Files"), target: nil, action: nil)
        all.isHidden = true
        let repository = self.repository
        let chooser = ClosureTarget { [weak alert] in
            FilePicker.presentTree(title: L("Export One Folder"), message: L("Folders in this commit."),
                                   repository: repository, rev: rev, in: alert?.window) { picked in
                guard let picked else { return }
                chosen = picked
                folder.stringValue = picked
                all.isHidden = false
            }
        }
        let clear = ClosureTarget {
            chosen = nil
            folder.stringValue = L("All files")
            all.isHidden = true
        }
        choose.target = chooser
        choose.action = #selector(ClosureTarget.fire)
        all.target = clear
        all.action = #selector(ClosureTarget.fire)
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Format:")), format],
            [NSTextField(labelWithString: L("Top folder:")), top],
            [NSTextField(labelWithString: L("Folder:")), NSStackView(views: [folder, choose, all])],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.rowSpacing = 8
        grid.frame = NSRect(x: 0, y: 0, width: 420, height: 84)
        alert.accessoryView = grid
        let response = await present(alert)
        _ = (chooser, clear)
        guard response == .alertFirstButtonReturn else { return nil }
        let picked = Self.archiveFormats[max(0, format.indexOfSelectedItem)].format
        defaults.set(picked, forKey: "archiveFormat")
        let prefix = top.stringValue.trimmingCharacters(in: CharacterSet.whitespaces.union(CharacterSet(charactersIn: "/")))
        return ArchiveOptions(format: picked, prefix: prefix, folder: chosen)
    }

    /// ファイル名と先頭のフォルダに使える名前（`/` と空白などは `-` に）。
    nonisolated static func archiveName(_ text: String) -> String {
        var out = ""
        for c in text.unicodeScalars {
            if CharacterSet.alphanumerics.contains(c) || c == "." || c == "_" || c == "-" {
                out.unicodeScalars.append(c)
            } else if !out.hasSuffix("-") {
                out.append("-")
            }
        }
        let trimmed = out.trimmingCharacters(in: CharacterSet(charactersIn: "-."))
        return trimmed.isEmpty ? "archive" : trimmed
    }
}
