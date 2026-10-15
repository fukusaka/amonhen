import QuartzCore

/// 1 フレームの中の処理ごとの時間（`--measure-scroll` のときだけ集める）。FE とコアの境目のどこが重いかを見る。
enum FrameProfile {
    /// 集めるか。計測の間だけ true にする。
    nonisolated(unsafe) static var enabled = false
    nonisolated(unsafe) private static var current: [String: CFTimeInterval] = [:]
    nonisolated(unsafe) private static var frames: [[String: CFTimeInterval]] = []
    nonisolated(unsafe) private static var order: [String] = []

    /// `body` にかかった時間を `name` に足す。
    @inline(__always)
    static func measure<R>(_ name: String, _ body: () throws -> R) rethrows -> R {
        guard enabled else { return try body() }
        let start = CACurrentMediaTime()
        defer {
            if current[name] == nil, !order.contains(name) { order.append(name) }
            current[name, default: 0] += CACurrentMediaTime() - start
        }
        return try body()
    }

    /// 裏のスレッドで測った時間などを `name` に足す（メインスレッドで呼ぶ）。
    static func add(_ name: String, _ duration: CFTimeInterval) {
        guard enabled else { return }
        if current[name] == nil, !order.contains(name) { order.append(name) }
        current[name, default: 0] += duration
    }

    /// 1 フレームの区切り。
    static func endFrame() {
        guard enabled else { return }
        frames.append(current)
        current = [:]
    }

    static func reset() {
        current = [:]
        frames = []
        order = []
    }

    /// 処理ごとの p50 / p95（ミリ秒）。
    static func summary() -> String {
        order.map { name in
            let values = frames.map { $0[name] ?? 0 }.sorted()
            guard !values.isEmpty else { return name }
            let at = { (p: Double) in values[min(values.count - 1, Int(Double(values.count) * p))] * 1000 }
            return String(format: "  %-28@ p50 %.3f  p95 %.3f ms", name as NSString, at(0.5), at(0.95))
        }.joined(separator: "\n")
    }
}
