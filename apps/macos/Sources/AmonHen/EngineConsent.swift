import AppKit
import AmonhenKit

/// SSH 先にエンジンを送る前の、利用者の承認（コアの `amonhen_remote::ssh::EngineConsent`）。コアは送ってよいかを
/// 決めていない SSH 先（`HostOptions.push` が nil）には送らず、エラーの文の終わりに `[amonhen:send-engine {JSON}]`
/// を載せて返す。ここで利用者に聞き、送るならその中身（`hash`）への承認を添えてつなぎ直す。初めて送るときも、版が
/// 変わって送り直すときも聞く。
nonisolated struct EngineConsent: Decodable, Sendable {
    /// SSH 先（`user@host:port`）。
    let host: String
    /// SSH 先の OS と CPU（`uname -sm`）。
    let platform: String
    /// 送るエンジンの大きさ（バイト）。
    let size: UInt64
    /// 送るエンジンの中身のハッシュ。承認はこれに対して。
    let hash: String
    /// 前に送ったエンジンを新しい版に入れ替える。
    let update: Bool

    private static let mark = "[amonhen:send-engine "

    /// エラーの文から取り出す。
    static func find(in message: String) -> EngineConsent? {
        guard let start = message.range(of: mark)?.upperBound,
              let end = message[start...].firstIndex(of: "}") else { return nil }
        return try? JSONDecoder().decode(EngineConsent.self, from: Data(message[start...end].utf8))
    }

    /// エラーの文から印を除いたもの。
    static func strip(_ message: String) -> String {
        guard let range = message.range(of: mark) else { return message }
        return message[..<range.lowerBound].trimmingCharacters(in: .whitespaces)
    }

    /// SSH 先（`location`）につなぐ。`attempt` は SSH 先ごとの選択の JSON を受け取ってつなぐ（裏のスレッドで呼ぶ）。
    /// エンジンを送る承認が要れば利用者に聞き（メインスレッドのダイアログを待つ）、送るならつなぎ直す。送らないなら
    /// その SSH 先の「送る」を切り（Settings > SSH Hosts）、送れない理由で失敗する。
    nonisolated static func connecting<T>(_ location: String, _ attempt: (String?) throws -> T) throws -> T {
        do {
            return try attempt(Settings.hostOptionsJSON(for: location))
        } catch {
            guard EngineConsent.find(in: "\(error)") != nil, let host = Settings.sshHost(of: location) else { throw error }
            // 同じ SSH 先を同時に開いても、聞くのは 1 回（後のものは、前の答えの後でつなぎ直す）。メインスレッドは
            // 待たない（裏のスレッドが鍵を持ったままメインスレッドのダイアログを待つので）。
            let locks = !Thread.isMainThread
            if locks { asking.lock() }
            defer { if locks { asking.unlock() } }
            do {
                return try attempt(Settings.hostOptionsJSON(for: location))
            } catch {
                guard let consent = EngineConsent.find(in: "\(error)") else { throw error }
                let answer = ask(consent, host: host)
                var options = Settings.hostOptions(for: host)
                switch answer {
                case .dontSend:
                    options.push = false
                    Settings.storeHostOptions(options, for: host)
                case .always:
                    options.push = true
                    Settings.storeHostOptions(options, for: host)
                case .once:
                    options.sendEngine = consent.hash
                }
                // 送らないなら、コアが送り込みを止めてある理由で失敗する。
                do {
                    return try attempt(options.json)
                } catch {
                    throw AmonhenError(description: strip("\(error)"))
                }
            }
        }
    }

    private static let asking = NSLock()

    enum Answer { case once, always, dontSend }

    /// 利用者に聞く（メインスレッドのダイアログ）。裏のスレッドからはダイアログが閉じるまで待つ。
    nonisolated static func ask(_ consent: EngineConsent, host: String) -> Answer {
        if Thread.isMainThread { return MainActor.assumeIsolated { present(consent, host: host) } }
        let done = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var answer = Answer.dontSend
        DispatchQueue.main.async {
            answer = MainActor.assumeIsolated { present(consent, host: host) }
            done.signal()
        }
        done.wait()
        return answer
    }

    @MainActor
    private static func present(_ consent: EngineConsent, host: String) -> Answer {
        let alert = NSAlert()
        let size = String(format: "%.1f", Double(consent.size) / 1e6)
        alert.messageText = consent.update
            ? L("Send a new version of the engine to %@?", host)
            : L("Send the engine to %@?", host)
        alert.informativeText = L(
            "To open repositories on this host, Amon Hen puts its engine (amonhen, %@ MB, %@) on the host and runs it there. If you don’t send it, Amon Hen won’t send one to this host until you turn it back on in Settings > SSH Hosts.",
            size, consent.platform)
        alert.addButton(withTitle: L("Send"))
        alert.addButton(withTitle: L("Don’t Send"))
        alert.showsSuppressionButton = true
        alert.suppressionButton?.title = L("Send without asking to this host from now on")
        NSApp.activate()
        guard alert.runModal() == .alertFirstButtonReturn else { return .dontSend }
        return alert.suppressionButton?.state == .on ? .always : .once
    }
}
