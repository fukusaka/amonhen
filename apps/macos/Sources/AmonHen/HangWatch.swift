import Foundation

/// メインスレッドが止まった瞬間の記録（開発用。`AMONHEN_HANG_LOG=<フォルダ>` を付けて起動したときだけ動く）。
/// 裏のスレッドから 50 ms ごとにメインスレッドへ合図を送り、0.15 秒返事がなければ、その最中のメインスレッドを
/// `sample` で 1 秒採って `hang-<時刻>.txt` に残し、止まっていた時間を `hangs.log` に書く。
enum HangWatch {
    nonisolated static func startIfRequested() {
        guard let path = ProcessInfo.processInfo.environment["AMONHEN_HANG_LOG"] else { return }
        let dir = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let pid = ProcessInfo.processInfo.processIdentifier
        let log = dir.appendingPathComponent("hangs.log")
        Thread.detachNewThread {
            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            while true {
                let started = Date()
                let answered = DispatchSemaphore(value: 0)
                DispatchQueue.main.async { answered.signal() }
                if answered.wait(timeout: .now() + 0.15) == .timedOut {
                    let stamp = formatter.string(from: started)
                    let sample = Process()
                    sample.executableURL = URL(fileURLWithPath: "/usr/bin/sample")
                    sample.arguments = ["\(pid)", "1", "1", "-file", dir.appendingPathComponent("hang-\(stamp).txt").path]
                    sample.standardOutput = FileHandle.nullDevice
                    sample.standardError = FileHandle.nullDevice
                    try? sample.run()
                    answered.wait()
                    let ms = Date().timeIntervalSince(started) * 1000
                    let line = "\(stamp) \(String(format: "%.0f", ms)) ms\n"
                    if let handle = try? FileHandle(forWritingTo: log) {
                        handle.seekToEndOfFile()
                        handle.write(Data(line.utf8))
                        try? handle.close()
                    } else {
                        try? line.write(to: log, atomically: true, encoding: .utf8)
                    }
                }
                usleep(50_000)
            }
        }
    }
}
