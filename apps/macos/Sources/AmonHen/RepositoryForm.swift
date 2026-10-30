import AmonhenKit
import AppKit
import SwiftUI

/// リポジトリの形（コアの `repositoryForm`）: shallow clone・partial clone・sparse checkout・maintenance の登録。
nonisolated struct RepositoryForm: Decodable, Equatable, Sendable {
    struct Partial: Decodable, Equatable, Sendable {
        /// 足りない中身を取ってくる remote。
        let remote: String
        /// 取らずにおいたもの（`blob:none` など）。
        let filter: String?
    }

    struct Sparse: Decodable, Equatable, Sendable {
        /// cone mode（フォルダで選ぶ）。
        let cone: Bool
        /// cone mode なら取り出しているフォルダ、そうでなければパターン。
        let folders: [String]

        /// `path`（ファイル）を取り出しているか（コアの `SparseCheckout::contains` と同じ）。cone mode は、取り出す
        /// フォルダの下すべてと、根とその途中のフォルダの直下のファイル。cone mode でなければ、取り出しているとみなす。
        func contains(_ path: String) -> Bool {
            guard cone, let slash = path.lastIndex(of: "/") else { return true }
            let parent = String(path[..<slash])
            return folders.contains { folder in
                parent == folder || parent.hasPrefix(folder + "/") || folder.hasPrefix(parent + "/")
            }
        }
    }

    let shallow: Bool
    let partial: Partial?
    let sparse: Sparse?
    let maintenance: Bool
    let workingTree: Bool
    let gitVersion: String?
    /// `git fetch --refetch` が使える（git 2.36 から）。
    let canFetchAllContents: Bool
    /// `git sparse-checkout` が使える（git 2.25 から）。
    let canSparseCheckout: Bool
    /// `git maintenance start` が使える（git 2.31 から）。
    let canMaintain: Bool

    /// 短い言い方と、その説明（Repository Settings の git の行に並べる）。ふつうの clone なら空。
    var items: [(title: String, help: String)] {
        var items: [(String, String)] = []
        if shallow {
            items.append(("shallow clone", L("Older history isn’t fetched. Fetch more in Fetching and Folders, or at the end of History.")))
        }
        if let partial {
            items.append((Self.partialTitle(partial),
                          L("Partial clone: file contents are fetched from %@ when they’re needed.", partial.remote)))
        }
        if let sparse {
            items.append((L("Some folders only"), Self.sparseHelp(sparse)))
        }
        if maintenance {
            items.append(("maintenance", L("Registered with git maintenance, which tidies it up in the background.")))
        }
        return items
    }

    static func partialTitle(_ partial: Partial) -> String {
        partial.filter.map { L("Contents fetched when needed (%@)", $0) } ?? L("Contents fetched when needed")
    }

    static func sparseHelp(_ sparse: Sparse) -> String {
        sparse.cone ? L("Sparse checkout: only %ld folders are checked out.", sparse.folders.count)
            : L("Sparse checkout: only the files matching its patterns are checked out.")
    }

    /// git が古くて使えないときの理由。
    @MainActor static func tooOld(_ needed: String, model: RepositorySettingsModel) -> String {
        L("Needs git %@ or later. This repository’s git is %@%@.", needed, model.gitVersion ?? "?", model.host.map { " (\($0))" } ?? "")
    }
}

/// 操作の行: 左に今の様子と、その下に 1 文の説明、右にボタン。使えなければボタンを止め、⚠ と理由を出す。
private struct ActionRow<Controls: View>: View {
    let title: String
    let help: String
    var disabledReason: String?
    @ViewBuilder let controls: () -> Controls

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(title)
                Spacer()
                controls().disabled(disabledReason != nil)
            }
            if !help.isEmpty {
                Text(help).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.horizontal, 8)
            }
            if let disabledReason {
                Text("⚠ " + disabledReason).foregroundStyle(.orange).padding(.horizontal, 8)
            }
        }
        .padding(.vertical, 6)
    }
}

/// 取得とフォルダ（docs/ux.md の 4.5）: shallow clone の履歴、partial clone の中身、sparse checkout のフォルダ。
/// そのリポジトリにあるものだけを出す（フォルダは作業ツリーがあればいつも）。最後に、git maintenance への登録。
struct FetchingPage: View {
    let model: RepositorySettingsModel

    var body: some View {
        Form {
            if let form = model.form {
                if form.shallow { historySection }
                if let partial = form.partial { contentsSection(form, partial) }
                if form.workingTree { foldersSection(form) }
                if !form.shallow, form.partial == nil, !form.workingTree {
                    Text(L("This repository has all of its history and contents.")).foregroundStyle(.secondary)
                }
                maintenanceSection(form)
            } else {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity)
            }
        }
        .formStyle(.grouped)
        .disabled(model.operationRunning)
        .overlay(alignment: .bottomTrailing) {
            if model.operationRunning { ProgressView().controlSize(.small).padding() }
        }
    }

    // MARK: - History（shallow clone）

    private var historySection: some View {
        Section(L("History")) {
            ActionRow(title: L("Older history isn’t fetched (shallow clone)"),
                      help: L("Fetch more of it from the remote, as at the end of History.")) {
                Menu(L("Fetch More")) {
                    ForEach(ShallowHistoryBand.moreChoices, id: \.commits) { choice in
                        Button(choice.title) { model.runOperation(["op": "deepen", "commits": choice.commits]) }
                    }
                    Button(L("Other…")) {
                        ShallowHistoryBand.askDeepen(in: NSApp.keyWindow) { commits in
                            model.runOperation(["op": "deepen", "commits": commits])
                        }
                    }
                    Divider()
                    Button(L("Up to a Date…")) {
                        ShallowHistoryBand.askDate(in: NSApp.keyWindow, boundary: nil) { since in
                            model.runOperation(["op": "deepen", "since": since])
                        }
                    }
                }
                .help(L("Fetch older commits from the remote (git fetch --deepen)"))
                .fixedSize()
                Button(L("Fetch All")) { model.runOperation(["op": "unshallow"]) }
                    .help(L("Fetch the whole history from the remote (git fetch --unshallow)"))
            }
        }
    }

    // MARK: - Contents（partial clone）

    private func contentsSection(_ form: RepositoryForm, _ partial: RepositoryForm.Partial) -> some View {
        Section(L("Contents")) {
            ActionRow(title: RepositoryForm.partialTitle(partial),
                      help: L("Diffs, blame and contents: searches fetch what they need from %@.", partial.remote),
                      disabledReason: form.canFetchAllContents ? nil : RepositoryForm.tooOld("2.36", model: model)) {
                Button(L("Fetch All Contents…")) { fetchAllContents(partial) }
            }
        }
    }

    private func fetchAllContents(_ partial: RepositoryForm.Partial) {
        let alert = NSAlert()
        alert.messageText = L("Fetch All Contents?")
        alert.informativeText = L("Fetches the contents of every file in the history from %@ (git fetch --refetch). In a large repository this can take long and use a lot of disk. Afterwards it’s no longer a partial clone.", partial.remote)
        alert.addButton(withTitle: L("Fetch All Contents"))
        alert.addButton(withTitle: L("Cancel"))
        let run = { (response: NSApplication.ModalResponse) in
            if response == .alertFirstButtonReturn { model.runOperation(["op": "fetchAllContents"]) }
        }
        if let window = NSApp.keyWindow { alert.beginSheetModal(for: window, completionHandler: run) } else { run(alert.runModal()) }
    }

    // MARK: - Maintenance

    /// git maintenance への登録（コアの `maintenance`）。入れると `git maintenance start`、切ると `unregister`
    /// （予定はほかのリポジトリも使うので残る）。登録してあれば、アプリは commit-graph を書かない（毎時 git が書く）。
    private func maintenanceSection(_ form: RepositoryForm) -> some View {
        Section(L("Maintenance")) {
            ActionRow(title: L("Keep this repository maintained in the background"),
                      help: model.host.map { L("Every hour git prefetches and writes the commit-graph; it’s set in the global git config and the scheduler on %@ (git maintenance).", $0) }
                          ?? L("Every hour git prefetches and writes the commit-graph; it’s set in your global git config and launchd (git maintenance)."),
                      disabledReason: form.canMaintain ? nil : RepositoryForm.tooOld("2.31", model: model)) {
                Toggle("", isOn: Binding(get: { form.maintenance },
                                         set: { model.runOperation(["op": "maintenance", "on": $0]) }))
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
            }
        }
    }

    // MARK: - Folders（sparse checkout）

    @ViewBuilder
    private func foldersSection(_ form: RepositoryForm) -> some View {
        let tooOld = form.canSparseCheckout ? nil : RepositoryForm.tooOld("2.25", model: model)
        Section(L("Folders")) {
            if let sparse = form.sparse, sparse.cone {
                ActionRow(title: L("Only these folders are checked out"),
                          help: L("Files at the top, and directly in the folders above these, are checked out too."),
                          disabledReason: tooOld) {
                    Button(L("Check Out All Folders…")) { checkOutAll() }
                }
                ForEach(sparse.folders, id: \.self) { folder in
                    HStack(spacing: 6) {
                        Image(systemName: "folder").foregroundStyle(.secondary)
                        Text(folder).font(.system(.body, design: .monospaced)).textSelection(.enabled)
                        Spacer()
                        Button { setFolders(sparse.folders.filter { $0 != folder }) } label: { Image(systemName: "minus") }
                            .buttonStyle(.borderless)
                            .help(L("Stop checking out this folder"))
                            .disabled(tooOld != nil)
                    }
                }
                HStack {
                    Button { addFolder(to: sparse.folders) } label: { Image(systemName: "plus") }
                        .help(L("Check out one more folder"))
                        .disabled(tooOld != nil)
                    Spacer()
                }
            } else if let sparse = form.sparse {
                // cone mode でないパターンは、ここでは変えない（git 2.37 から非推奨）。やめることだけできる。
                ActionRow(title: L("Only the files matching these patterns are checked out"),
                          help: L("The patterns aren’t in cone mode, so Amon Hen doesn’t change them."),
                          disabledReason: tooOld) {
                    Button(L("Check Out All Folders…")) { checkOutAll() }
                }
                ForEach(sparse.folders, id: \.self) { pattern in
                    Text(pattern).font(.system(.body, design: .monospaced)).textSelection(.enabled)
                }
            } else {
                ActionRow(title: L("All folders are checked out"),
                          help: L("In a large repository, you can check out only the folders you work in (sparse checkout)."),
                          disabledReason: tooOld) {
                    Button(L("Check Out Only Some Folders…")) { addFolder(to: []) }
                }
            }
        }
    }

    private func setFolders(_ folders: [String]) {
        model.runOperation(["op": "sparseCheckout", "folders": folders])
    }

    /// HEAD の木からフォルダを 1 つ選んで、取り出すフォルダに足す（sparse checkout でなければ、それだけにする）。
    private func addFolder(to folders: [String]) {
        let first = folders.isEmpty
        FilePicker.presentTree(
            title: first ? L("Check Out Only Some Folders") : L("Check Out One More Folder"),
            message: first ? L("Only this folder is checked out, with the files at the top. Others are removed from disk until you check them out again; add more folders here later.")
                : L("Folders in the latest commit, including ones not checked out."),
            repository: model.repository, in: NSApp.keyWindow
        ) { folder in
            guard let folder, !folders.contains(folder) else { return }
            setFolders(folders + [folder])
        }
    }

    private func checkOutAll() {
        let alert = NSAlert()
        alert.messageText = L("Check Out All Folders?")
        alert.informativeText = L("Every folder is checked out again (git sparse-checkout disable). In a large repository this can take long and use a lot of disk.")
        alert.addButton(withTitle: L("Check Out All"))
        alert.addButton(withTitle: L("Cancel"))
        let run = { (response: NSApplication.ModalResponse) in
            if response == .alertFirstButtonReturn { model.runOperation(["op": "disableSparseCheckout"]) }
        }
        if let window = NSApp.keyWindow { alert.beginSheetModal(for: window, completionHandler: run) } else { run(alert.runModal()) }
    }
}
