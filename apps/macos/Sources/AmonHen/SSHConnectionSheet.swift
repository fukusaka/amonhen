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
    /// sh で読めなければ（つながったのに sh が動かなければ）、Windows とみて PowerShell で読む。
    static func check(_ host: String) async -> (text: String, ok: Bool) {
        await Task.detached(priority: .userInitiated) { () -> (String, Bool) in
            func run(_ command: String) -> (status: Int32, out: String, err: String)? {
                let process = Process()
                process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
                process.arguments = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, command]
                let out = Pipe(), err = Pipe()
                process.standardOutput = out
                process.standardError = err
                process.standardInput = FileHandle.nullDevice
                guard (try? process.run()) != nil else { return nil }
                let data = out.fileHandleForReading.readDataToEndOfFile()
                let errorData = err.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                return (process.terminationStatus, String(decoding: data, as: UTF8.self), String(decoding: errorData, as: UTF8.self))
            }
            guard var result = run("git --version 2>/dev/null; command -v amonhen >/dev/null 2>&1 && echo engine; true") else {
                return (L("Couldn’t run ssh"), false)
            }
            // ssh そのものの失敗は 255。
            if result.status != 0, result.status != 255,
               let windows = run(powerShell("git --version; if (Get-Command amonhen -CommandType Application -ErrorAction SilentlyContinue) { 'engine' }")),
               windows.status == 0 {
                result = windows
            }
            let lines = result.out.split(whereSeparator: \.isNewline).map(String.init)
            guard result.status == 0 else {
                let message = result.err.trimmingCharacters(in: .whitespacesAndNewlines)
                return (message.isEmpty ? L("Couldn’t connect (exit %ld)", Int(result.status)) : message, false)
            }
            let git = lines.first { $0.hasPrefix("git version") }.map { String($0.dropFirst("git version ".count)) }
            let engine = lines.contains("engine")
            var text = git.map { L("Reachable: git %@", $0) } ?? L("Reachable, but git wasn’t found on the host")
            text += engine ? L(", engine found") : L(", engine not installed (Amon Hen sends one)")
            return (text, git != nil)
        }.value
    }

    /// PowerShell の `script` を、Windows の SSH 先のどの既定のシェル（cmd.exe・pwsh・Windows PowerShell）でも同じ意味に
    /// なるコマンドに（`-EncodedCommand` は UTF-16LE の base64）。
    nonisolated private static func powerShell(_ script: String) -> String {
        let bytes = script.utf16.flatMap { [UInt8($0 & 0xff), UInt8($0 >> 8)] }
        return "powershell -NoProfile -NonInteractive -EncodedCommand \(Data(bytes).base64EncodedString())"
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

    /// ホストの中を見て選ぶ（SSH 先のエンジンにつないで読む）。
    func choose() {
        guard !trimmedHost.isEmpty else { return }
        FilePicker.present(title: L("Choose a Repository on %@", trimmedHost), message: L("Click a folder to open it, and choose a repository (green)."),
                                 kind: .folder, host: trimmedHost, initial: trimmedPath) { [weak self] chosen in
            guard let self, let chosen else { return }
            host = chosen.host
            path = chosen.shown
        }
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
