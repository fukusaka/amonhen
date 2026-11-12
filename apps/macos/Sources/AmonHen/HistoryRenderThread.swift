import Metal
import QuartzCore
import os

/// History のグラフと文字を描いて出す、ただ 1 本の描画のスレッド。メインスレッドは描くもの（インスタンスの列）を
/// 組み立てて渡すだけで、描き先を取り、GPU の命令を作り、画面に出すのはここだけで行う（Chrome のコンポジタ、
/// Flutter のラスタのスレッドと同じ分け方）。同じ層に 2 か所から描かないので、古い絵が後から出ることがない。
/// Core Animation の反映は待たずに出す。追いつかないときは、まだ描いていない古い頼みを捨てて最新だけを描く。
nonisolated final class HistoryRenderThread: @unchecked Sendable {
    /// 描く 1 枚。`encode` は描画のスレッドで、描き先のテクスチャへの命令を作る。
    struct Job: @unchecked Sendable {
        var layer: CAMetalLayer
        var size: CGSize
        var encode: (MTLTexture, MTLCommandBuffer, HistoryRenderThread) -> Void
        var onPresented: (@Sendable (CFTimeInterval) -> Void)?
        var onCompleted: (@Sendable (CFTimeInterval) -> Void)?
    }

    private let queue = DispatchQueue(label: "amonhen.history-render", qos: .userInteractive)
    private let commandQueue: MTLCommandQueue
    /// 層ごとの、まだ描いていない最新の頼みと、描画のスレッドに頼んであるか。
    private let pending = OSAllocatedUnfairLock(initialState: (jobs: [ObjectIdentifier: Job](), scheduled: false))
    /// インスタンスを入れるバッファ。層ごとに 3 つを順に使う（描き先も 3 枚なので、GPU が読んでいる間に
    /// 書き換えない）。描画のスレッドだけが触る。
    private var buffers: [ObjectIdentifier: (list: [MTLBuffer], index: Int)] = [:]

    init?(device: MTLDevice) {
        guard let commandQueue = device.makeCommandQueue() else { return nil }
        self.commandQueue = commandQueue
    }

    /// `job.layer` に描くよう頼む（同じ層のまだ描いていない頼みは捨てる）。どのスレッドから呼んでもよい。
    func submit(_ job: Job) {
        let schedule = pending.withLock { state in
            state.jobs[ObjectIdentifier(job.layer)] = job
            defer { state.scheduled = true }
            return !state.scheduled
        }
        if schedule { queue.async { self.drain() } }
    }

    private func drain() {
        let jobs = pending.withLock { state in
            defer { state = ([:], false) }
            return Array(state.jobs.values)
        }
        for job in jobs { draw(job) }
    }

    private func draw(_ job: Job) {
        guard job.size.width > 0, job.size.height > 0, let drawable = job.layer.nextDrawable(),
              let commands = commandQueue.makeCommandBuffer() else { return }
        job.encode(drawable.texture, commands, self)
        if let onPresented = job.onPresented {
            drawable.addPresentedHandler { onPresented($0.presentedTime) }
        }
        if let onCompleted = job.onCompleted {
            commands.addCompletedHandler { onCompleted($0.gpuEndTime) }
        }
        commands.present(drawable)
        commands.commit()
    }

    /// `layer` の描画に使う、`length` バイト以上のバッファ（描画のスレッドで `encode` から呼ぶ）。
    func buffer(for layer: CAMetalLayer, length: Int, device: MTLDevice) -> MTLBuffer {
        let key = ObjectIdentifier(layer)
        var entry = buffers[key] ?? ([], 0)
        if entry.list.count < 3 {
            entry.list.append(device.makeBuffer(length: length, options: .storageModeShared)!)
            entry.index = entry.list.count - 1
        } else {
            entry.index = (entry.index + 1) % entry.list.count
        }
        if entry.list[entry.index].length < length {
            entry.list[entry.index] = device.makeBuffer(length: length * 2, options: .storageModeShared)!
        }
        buffers[key] = entry
        return entry.list[entry.index]
    }
}
