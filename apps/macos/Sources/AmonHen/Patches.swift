import AmonhenKit
import AppKit
import UniformTypeIdentifiers

/// パッチ: コミットをパッチに書き出す（git format-patch）、パッチを当てる（メールの形は git am、ただの diff は
/// git apply）。パッチのファイルはいつも手元に置く（SSH 先のリポジトリでも、コアが中身をやりとりする）。
extension Actions {
    /// パッチのファイルとして受け取る拡張子（ドロップと、ファイルを選ぶパネル）。
    static let patchExtensions: Set<String> = ["patch", "diff", "mbox", "eml"]

    /// `oids`（古い順）をパッチにする。形（1 コミット 1 ファイルか 1 つの mbox か）・送り状・版を選んでもらってから、
    /// 書き出す場所を選んでもらう。書いたら Finder で見せる。
    func savePatches(_ oids: [String]) {
        guard let first = oids.first else { return }
        let repository = self.repository
        Task {
            let subject = await Task.detached { (try? repository.commitInfo(oid: first))?.message }.value?
                .split(separator: "\n").first.map(String.init)
            guard let options = await askPatchOptions(count: oids.count) else { return }
            var command: [String: Any] = ["op": "formatPatch", "oids": oids, "coverLetter": options.coverLetter]
            if let version = options.version { command["reroll"] = version }
            if options.single {
                let panel = NSSavePanel()
                panel.allowedContentTypes = [UTType(filenameExtension: "mbox") ?? .data]
                panel.nameFieldStringValue = Self.patchFileName(subject ?? "patches") + ".mbox"
                panel.canCreateDirectories = true
                guard panel.runModal() == .OK, let url = panel.url else { return }
                command["file"] = url.path
            } else {
                let panel = NSOpenPanel()
                panel.canChooseFiles = false
                panel.canChooseDirectories = true
                panel.canCreateDirectories = true
                panel.prompt = L("Save")
                panel.message = L("Choose the folder for the patches. Files with the same names are replaced.")
                guard panel.runModal() == .OK, let url = panel.url else { return }
                command["dir"] = url.path
            }
            struct Written: Decodable { let files: [String] }
            do {
                let json = try Repository.json(command)
                RunningOperations.begin()
                let written = await Task.detached(priority: .userInitiated) {
                    Result { try repository.info(json: json, as: Written.self) }
                }.value
                RunningOperations.end()
                let files = try written.get().files
                NSWorkspace.shared.activateFileViewerSelecting(files.map { URL(fileURLWithPath: $0) })
            } catch {
                await fail(OperationFailure(error, command: command))
            }
        }
    }

    private struct PatchOptions {
        var single: Bool
        var coverLetter: Bool
        var version: Int?
    }

    /// 書き出しの形を選んでもらう。前の選び方（形と送り状）を覚えておく。キャンセルなら nil。
    @MainActor
    private func askPatchOptions(count: Int) async -> PatchOptions? {
        let defaults = UserDefaults.standard
        let alert = NSAlert()
        alert.messageText = count == 1 ? L("Save as Patch") : L("Save %ld Commits as Patches", count)
        alert.informativeText = L("Numbering, recipients and sign-offs follow git’s format.* settings.")
        alert.addButton(withTitle: L("Save…"))
        alert.addButton(withTitle: L("Cancel"))
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        format.addItems(withTitles: [L("One file per commit"), L("One mbox file")])
        format.selectItem(at: defaults.bool(forKey: "patchSingleFile") ? 1 : 0)
        let cover = NSButton(checkboxWithTitle: L("Cover letter"), target: nil, action: nil)
        cover.state = defaults.bool(forKey: "patchCoverLetter") ? .on : .off
        let version = NSTextField(string: "")
        version.placeholderString = "1"
        version.widthAnchor.constraint(equalToConstant: 48).isActive = true
        let versionRow = NSStackView(views: [NSTextField(labelWithString: L("Version (-v):")), version])
        let formatRow = NSStackView(views: [NSTextField(labelWithString: L("Format:")), format])
        let stack = NSStackView(views: [formatRow, cover, versionRow])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: 300, height: 92)
        alert.accessoryView = stack
        guard await present(alert) == .alertFirstButtonReturn else { return nil }
        let single = format.indexOfSelectedItem == 1
        defaults.set(single, forKey: "patchSingleFile")
        defaults.set(cover.state == .on, forKey: "patchCoverLetter")
        let number = Int(version.stringValue.trimmingCharacters(in: .whitespaces))
        return PatchOptions(single: single, coverLetter: cover.state == .on, version: number.flatMap { $0 > 1 ? $0 : nil })
    }

    /// git format-patch と同じ考えのファイル名（英数字と . _ のほかは - にまとめる）。
    nonisolated static func patchFileName(_ subject: String) -> String {
        var out = ""
        for c in subject.unicodeScalars {
            if CharacterSet.alphanumerics.contains(c) && c.isASCII || c == "." || c == "_" {
                out.unicodeScalars.append(c)
            } else if !out.hasSuffix("-") {
                out.append("-")
            }
        }
        let trimmed = out.trimmingCharacters(in: CharacterSet(charactersIn: "-."))
        return trimmed.isEmpty ? "patches" : String(trimmed.prefix(52))
    }

    /// パッチのファイルを選んで当てる（Repository › Apply Patch…）。
    func chooseAndApplyPatches() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.message = L("Choose patch files (.patch, .diff, .mbox, .eml).")
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
        applyPatches(panel.urls)
    }

    /// パッチのファイルを読んで、何を変えるかと当たるかを見せてから当てる。名前の順につなぐ（0001-… から）。
    func applyPatches(_ urls: [URL]) {
        let paths = urls.map(\.path).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        let repository = self.repository
        Task {
            let result: Result<PatchSummary, Error>
            do {
                let json = try Repository.json(["op": "readPatch", "paths": paths])
                result = await Task.detached { Result { try repository.info(json: json, as: PatchSummary.self) } }.value
            } catch {
                result = .failure(error)
            }
            switch result {
            case let .failure(error):
                await alert(L("Can’t Read the Patch"), "\(error)")
            case let .success(summary):
                if summary.patches.allSatisfy(\.files.isEmpty) {
                    await alert(L("Can’t Read the Patch"), L("There are no changes in the patch."))
                } else {
                    await confirmApply(summary, paths: paths)
                }
            }
        }
    }

    /// 当てる前の確認: パッチごとの件名・作者・ファイル、当たるか、当て方の選び方。
    @MainActor
    private func confirmApply(_ summary: PatchSummary, paths: [String]) async {
        let alert = NSAlert()
        let names = paths.map { ($0 as NSString).lastPathComponent }
        let branch = currentBranch ?? "HEAD"
        if summary.isMailbox {
            alert.messageText = summary.patches.count == 1
                ? L("Apply “%@”?", summary.patches[0].subject)
                : L("Apply %ld Patches?", summary.patches.count)
            alert.informativeText = L("Each patch becomes a commit on %@ (git am). You can Undo it.", branch)
        } else {
            alert.messageText = names.count == 1 ? L("Apply %@?", names[0]) : L("Apply %ld Patches?", names.count)
            alert.informativeText = L("The changes go into the working tree (git apply). You can Undo it.")
        }
        alert.addButton(withTitle: L("Apply"))
        alert.addButton(withTitle: L("Cancel"))

        let list = NSTextView()
        list.isEditable = false
        list.drawsBackground = false
        list.textStorage?.setAttributedString(Self.patchList(summary))
        let width: CGFloat = 440
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: width, height: 140))
        // 積み重ねの中では大きさを決めておかないと、高さが 0 になる。
        scroll.widthAnchor.constraint(equalToConstant: width).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 140).isActive = true
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        list.frame = scroll.bounds
        list.autoresizingMask = [.width]
        scroll.documentView = list

        let status = NSTextField(wrappingLabelWithString: summary.applies
            ? L("Applies cleanly.")
            : L("Doesn’t apply cleanly: %@", Self.firstLine(summary.error ?? "")))
        status.textColor = summary.applies ? .secondaryLabelColor : .systemOrange
        status.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        status.preferredMaxLayoutWidth = width

        var views: [NSView] = [scroll, status]
        let threeWay = NSButton(checkboxWithTitle: L("3-way merge where it doesn’t apply"), target: nil, action: nil)
        threeWay.toolTip = L("Leaves conflicts to resolve in Working Copy instead of stopping (--3way).")
        let index = NSButton(checkboxWithTitle: L("Stage the changes too (--index)"), target: nil, action: nil)
        if summary.isMailbox {
            // 初期値は git の am.threeWay（git の既定はオフ）。
            threeWay.state = summary.threeWay ? .on : .off
            views.append(threeWay)
        } else {
            views.append(contentsOf: [index, threeWay])
        }
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: width, height: stack.fittingSize.height)
        alert.accessoryView = stack
        guard await present(alert) == .alertFirstButtonReturn else { return }
        if summary.isMailbox {
            perform(["op": "am", "paths": paths, "threeWay": threeWay.state == .on])
        } else {
            perform(["op": "applyPatch", "paths": paths, "index": index.state == .on, "threeWay": threeWay.state == .on])
        }
    }

    /// 確認のシートの一覧: パッチごとに件名と作者、その下にファイルと増減。
    private static func patchList(_ summary: PatchSummary) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let bold = NSFont.boldSystemFont(ofSize: NSFont.smallSystemFontSize)
        let small = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        let mono = NSFont.monospacedSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        for patch in summary.patches where !(patch.subject.isEmpty && patch.files.isEmpty) {
            if !patch.subject.isEmpty {
                out.append(NSAttributedString(string: patch.subject, attributes: [.font: bold, .foregroundColor: NSColor.labelColor]))
                if let author = patch.author ?? patch.authorEmail {
                    out.append(NSAttributedString(string: "  " + author, attributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor]))
                }
                out.append(NSAttributedString(string: "\n"))
            }
            for file in patch.files {
                let name = file.oldPath.map { "\($0) → \(file.path)" } ?? file.path
                let change = file.binary ? "bin" : "+\(file.added) −\(file.removed)"
                let mark = switch file.status {
                case "added": "A"
                case "deleted": "D"
                case "renamed": "R"
                case "copied": "C"
                default: "M"
                }
                out.append(NSAttributedString(string: "  \(mark) \(name)  ", attributes: [.font: mono, .foregroundColor: NSColor.labelColor]))
                out.append(NSAttributedString(string: change + "\n", attributes: [.font: mono, .foregroundColor: NSColor.secondaryLabelColor]))
            }
        }
        return out
    }

    private static func firstLine(_ text: String) -> String {
        let lines = text.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
        // git の答えの最初の `error:` の行（なければ最初の行）。
        return lines.first { $0.hasPrefix("error:") }.map { String($0.dropFirst("error:".count)).trimmingCharacters(in: .whitespaces) }
            ?? lines.first ?? ""
    }
}
