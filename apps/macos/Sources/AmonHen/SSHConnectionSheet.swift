import AppKit
import SwiftUI

/// SSH 接続（SSH 先のリポジトリ）を加えるシート。リモートのシートと同じ形: 接続先（~/.ssh/config の名前か
/// user@host:port）、リポジトリのパス（「選ぶ…」でホストの中を見て選べる）、接続の確認（BatchMode の ssh で
/// git とエンジンの有無を読む）、「加えて開く」。ホストの行から開いたときは接続先が決まっている。
enum SSHConnectionSheet {
    static func present(in window: NSWindow, host fixedHost: String?, hosts: [String], initialHost: String?, initialPath: String,
                        onAdd: @escaping (String, String) -> Void) {
        let model = SSHConnectionModel(fixedHost: fixedHost, hosts: hosts, initialHost: initialHost, initialPath: initialPath, onAdd: onAdd)
        let host = NSHostingController(rootView: SSHConnectionView(model: model))
        let sheet = NSWindow(contentViewController: host)
        sheet.styleMask = [.titled]
        model.close = { [weak window, weak sheet] in
            guard let window, let sheet else { return }
            window.endSheet(sheet)
        }
        window.beginSheet(sheet)
    }

    /// 接続先を一覧から外す前の確認（リポジトリと、スキャンするフォルダの数を言う）。リモートの削除と同じ形。
    static func confirmRemove(host: String, repositories: Int, folders: Int) -> Bool {
        let alert = NSAlert()
        alert.messageText = L("Remove %@?", host)
        alert.informativeText = L("This removes its %ld repositories and %ld folders to scan from the list. Nothing on the host is touched.", repositories, folders)
        alert.addButton(withTitle: L("Delete"))
        alert.addButton(withTitle: L("Cancel"))
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// `host` につないで、git の版とエンジン（`amonhen`）の有無を読む。パスワードを聞くホストにはつながない（BatchMode）。
    static func check(_ host: String) async -> (text: String, ok: Bool) {
        await Task.detached(priority: .userInitiated) { () -> (String, Bool) in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
            process.arguments = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host,
                                 "git --version 2>/dev/null; command -v amonhen >/dev/null 2>&1 && echo engine"]
            let out = Pipe(), err = Pipe()
            process.standardOutput = out
            process.standardError = err
            process.standardInput = FileHandle.nullDevice
            guard (try? process.run()) != nil else { return (L("Couldn’t run ssh"), false) }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            let errorData = err.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            let lines = String(decoding: data, as: UTF8.self).split(separator: "\n").map(String.init)
            guard process.terminationStatus == 0 else {
                let message = String(decoding: errorData, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                return (message.isEmpty ? L("Couldn’t connect (exit %ld)", Int(process.terminationStatus)) : message, false)
            }
            let git = lines.first { $0.hasPrefix("git version") }.map { String($0.dropFirst("git version ".count)) }
            let engine = lines.contains("engine")
            var text = git.map { L("Reachable: git %@", $0) } ?? L("Reachable, but git wasn’t found on the host")
            text += engine ? L(", engine found") : L(", engine not installed (Amon Hen sends one)")
            return (text, git != nil)
        }.value
    }
}

@Observable
final class SSHConnectionModel {
    let fixedHost: String?
    let hosts: [String]
    let onAdd: (String, String) -> Void
    var close: () -> Void = {}
    var host: String
    var path: String
    var checking = false
    var checkResult: (text: String, ok: Bool)?

    init(fixedHost: String?, hosts: [String], initialHost: String?, initialPath: String, onAdd: @escaping (String, String) -> Void) {
        self.fixedHost = fixedHost
        self.hosts = hosts
        self.onAdd = onAdd
        host = fixedHost ?? initialHost ?? ""
        path = initialPath
    }

    var trimmedHost: String { host.trimmingCharacters(in: .whitespaces) }
    var trimmedPath: String { path.trimmingCharacters(in: .whitespaces) }
    var canAdd: Bool { !trimmedHost.isEmpty && !trimmedPath.isEmpty && !trimmedHost.contains(" ") && !trimmedHost.contains("/") && !checking }

    func check() {
        let host = trimmedHost
        guard !host.isEmpty else { return }
        checking = true
        checkResult = nil
        Task {
            checkResult = await SSHConnectionSheet.check(host)
            checking = false
        }
    }

    /// ホストの中を見て選ぶ（今までのフォルダの一覧。パスワードを聞くホストでは読めない）。
    func choose() {
        guard let chosen = SSHFolderChooser.run(
            title: L("Choose a Repository on %@", trimmedHost), message: L("Double-click a folder to go into it, and choose a repository (green)."),
            button: L("Choose"), fixedHost: trimmedHost.isEmpty ? nil : trimmedHost, hosts: hosts,
            initialHost: trimmedHost.isEmpty ? nil : trimmedHost, initialPath: trimmedPath.isEmpty ? "~" : trimmedPath)
        else { return }
        host = chosen.host
        path = chosen.path
    }

    func add() {
        guard canAdd else { return }
        onAdd(trimmedHost, trimmedPath)
        close()
    }
}

private struct SSHConnectionView: View {
    @Bindable var model: SSHConnectionModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(model.fixedHost.map { L("Add a Repository on %@", $0) } ?? L("Add SSH Connection")).font(.headline)
            Text(L("Connects over SSH. Use a host you can log in to without a password (with a key or ssh-agent)."))
                .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 8) {
                GridRow {
                    Text(L("Host")).gridColumnAlignment(.trailing)
                    HStack(spacing: 4) {
                        TextField("", text: $model.host, prompt: Text(L("user@host:port (or a name in ~/.ssh/config)")))
                            .labelsHidden().disabled(model.fixedHost != nil)
                        if model.fixedHost == nil, !model.hosts.isEmpty {
                            Menu {
                                ForEach(model.hosts, id: \.self) { h in Button(h) { model.host = h } }
                            } label: { Image(systemName: "chevron.down") }
                            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                            .help(L("Hosts in ~/.ssh/config and ones already in the list"))
                        }
                    }
                }
                GridRow {
                    Text(L("Path"))
                    HStack(spacing: 4) {
                        TextField("", text: $model.path, prompt: Text(L("Folder of the repository on the host"))).labelsHidden()
                        Button(L("Choose…")) { model.choose() }.disabled(model.trimmedHost.isEmpty)
                    }
                }
            }
            HStack(spacing: 8) {
                Button(L("Check Connection")) { model.check() }.disabled(model.trimmedHost.isEmpty || model.checking)
                if model.checking {
                    ProgressView().controlSize(.small)
                    Text(L("Connecting…")).foregroundStyle(.secondary)
                } else if let r = model.checkResult {
                    Text(r.text).foregroundStyle(r.ok ? Color.green : Color.red).lineLimit(3)
                }
                Spacer()
            }
            HStack {
                Spacer()
                Button(L("Cancel")) { model.close() }.keyboardShortcut(.cancelAction)
                Button(L("Add and Open")) { model.add() }.keyboardShortcut(.defaultAction).disabled(!model.canAdd)
            }
        }
        .padding(20)
        .frame(width: 520)
    }
}
