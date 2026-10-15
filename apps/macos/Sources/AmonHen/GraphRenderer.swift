import AmonhenKit
import CAmonhen
import Metal
import QuartzCore

/// History のグラフを Metal で描く。点・線・曲線を四角形のインスタンスにし、形は fragment
/// シェーダーの SDF（境界までの距離）で描いて、解析的にアンチエイリアスする。描画命令は 1 回。
final class GraphRenderer {
    struct Instance {
        var a: SIMD2<Float>
        var b: SIMD2<Float>
        /// 線は太さ、点は半径。
        var width: Float
        var color: UInt32
        var kind: UInt32
        /// 不透明度（詰めたレーンの線は薄くする）。
        var alpha: Float = 1

        static let line: UInt32 = 0
        static let circle: UInt32 = 1
        static let curve: UInt32 = 2
    }

    struct Uniforms {
        var viewport: SIMD2<Float>
        var scale: Float
        var pad: Float = 0
    }

    struct Metrics {
        /// History の行の高さ（件名と、作者・日時の 2 行）。
        var rowHeight: Float = 38
        var laneWidth: Float = 14
        var leftInset: Float = 10
        var nodeRadius: Float = 4
        var lineWidth: Float = 1.6
        var firstParentWidth: Float = 2.4
        /// 線だけのレーンの幅（魚眼で詰めたとき）。入りきらなければ 0 まで詰めて、薄い帯にする。
        var thinLaneWidth: Float = 3
        var minLaneWidth: Float = 7
        var minThinLaneWidth: Float = 0
    }

    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private var buffers: [MTLBuffer] = []
    private var bufferIndex = 0
    private var instances: [Instance] = []
    var metrics = Metrics()
    /// 描いたフレームと、描き先を取れずに飛ばしたフレームの数。
    private(set) var drawnFrames = 0
    private(set) var skippedFrames = 0

    /// 色は `color % palette.count` で選ぶ。0 は幹線。
    var palette: [SIMD4<Float>] = [
        [0.30, 0.55, 0.95, 1], [0.93, 0.45, 0.30, 1], [0.35, 0.75, 0.45, 1], [0.75, 0.45, 0.85, 1],
        [0.95, 0.70, 0.25, 1], [0.30, 0.75, 0.80, 1], [0.90, 0.40, 0.60, 1], [0.60, 0.60, 0.65, 1],
    ]

    init?() {
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
            return nil
        }
        self.device = device
        self.queue = queue
        do {
            let library = try device.makeLibrary(source: Self.shader, options: nil)
            let desc = MTLRenderPipelineDescriptor()
            desc.vertexFunction = library.makeFunction(name: "graph_vertex")
            desc.fragmentFunction = library.makeFunction(name: "graph_fragment")
            let color = desc.colorAttachments[0]!
            color.pixelFormat = .bgra8Unorm
            color.isBlendingEnabled = true
            color.sourceRGBBlendFactor = .one
            color.sourceAlphaBlendFactor = .one
            color.destinationRGBBlendFactor = .oneMinusSourceAlpha
            color.destinationAlphaBlendFactor = .oneMinusSourceAlpha
            pipeline = try device.makeRenderPipelineState(descriptor: desc)
        } catch {
            NSLog("Metal のパイプラインを作れない: \(error)")
            return nil
        }
    }

    // MARK: - レーンの横位置（魚眼）

    /// 今描いているレーンの位置と、向かっている位置（コアが見えている範囲から決める）。
    private var currentX: [Float] = []
    private var targetX: [Float] = []
    private var targetKey: (history: ObjectIdentifier, rows: Range<Int>, width: Float)?
    private var lastAdvance: CFTimeInterval = 0
    /// レーンの位置がまだ動いている（FE は止まるまで描き直す）。
    private(set) var isAnimating = false

    func laneX(_ lane: UInt16) -> Float {
        let i = Int(lane)
        let x = i < currentX.count ? currentX[i] : Float(i) * metrics.laneWidth + metrics.laneWidth / 2
        return metrics.leftInset - metrics.laneWidth / 2 + x
    }

    /// 見えている行 `rows` に合わせてレーンの位置を決め、`animate` なら今の位置から少しずつ近づける。
    private func updateLanes(history: History, rows: Range<Int>, width: Float, animate: Bool) {
        // 少し先の行まで含めて決め、スクロールで点が入ってくる直前に広げ始める。
        let margin = 4
        let window = max(0, rows.lowerBound - margin)..<min(history.rowCount, rows.upperBound + margin)
        let key = (ObjectIdentifier(history), window, width)
        let historyChanged = targetKey?.history != key.0
        if targetKey == nil || historyChanged || targetKey!.rows != window || targetKey!.width != width {
            let m = AmonhenLaneMetrics(
                full: metrics.laneWidth, thin: metrics.thinLaneWidth, min_full: metrics.minLaneWidth,
                min_thin: metrics.minThinLaneWidth,
                available: width - (metrics.leftInset - metrics.laneWidth / 2) - 4)
            history.lanePositions(rows: window, metrics: m, into: &targetX)
            targetKey = key
            // 別の History に替わったときは、動かさずにその位置から始める。
            if historyChanged || currentX.count != targetX.count { currentX = targetX }
        }
        let now = CACurrentMediaTime()
        let dt = min(Float(now - lastAdvance), 0.05)
        lastAdvance = now
        guard animate else {
            currentX = targetX
            isAnimating = false
            return
        }
        // 約 0.15 秒で近づく指数的な動き。
        let k = min(1, dt * 18)
        var moving = false
        for i in currentX.indices {
            let d = targetX[i] - currentX[i]
            if abs(d) < 0.05 {
                currentX[i] = targetX[i]
            } else {
                currentX[i] += d * k
                moving = true
            }
        }
        isAnimating = moving
    }

    func rowY(_ row: UInt32, scrollY: Float) -> Float {
        (Float(row) + 0.5) * metrics.rowHeight - scrollY
    }

    /// `scrollY` だけスクロールした位置から見える範囲を `layer` に描く。
    func draw(
        history: History, layer: CAMetalLayer, scrollY: Float,
        onCompleted: (@Sendable (_ gpuEnd: CFTimeInterval) -> Void)? = nil,
        onPresented: (@Sendable (CFTimeInterval) -> Void)?
    ) {
        let size = layer.bounds.size
        guard size.width > 0, size.height > 0, let drawable = layer.nextDrawable(),
              let commands = queue.makeCommandBuffer() else {
            skippedFrames += 1
            return
        }
        drawnFrames += 1
        encode(history: history, into: drawable.texture, size: size, scale: Float(layer.contentsScale),
               scrollY: scrollY, animate: true, commands: commands)
        if let onPresented {
            drawable.addPresentedHandler { onPresented($0.presentedTime) }
        }
        if let onCompleted {
            commands.addCompletedHandler { onCompleted($0.gpuEndTime) }
        }
        if layer.presentsWithTransaction {
            // 表のスクロールと同じ Core Animation のトランザクションで表示する（ずれを出さない）。
            commands.commit()
            commands.waitUntilScheduled()
            drawable.present()
        } else {
            commands.present(drawable)
            commands.commit()
        }
    }

    /// 画面に出さずに描いて画像にする（`--snapshot` で見た目を確かめるため）。
    func image(history: History, size: CGSize, scale: CGFloat, scrollY: Float) -> CGImage? {
        let width = Int(size.width * scale), height = Int(size.height * scale)
        guard width > 0, height > 0 else { return nil }
        let desc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm, width: width, height: height, mipmapped: false)
        desc.usage = [.renderTarget, .shaderRead]
        desc.storageMode = .shared
        guard let texture = device.makeTexture(descriptor: desc), let commands = queue.makeCommandBuffer() else {
            return nil
        }
        encode(history: history, into: texture, size: size, scale: Float(scale), scrollY: scrollY, animate: false,
               commands: commands)
        commands.commit()
        commands.waitUntilCompleted()

        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        texture.getBytes(&pixels, bytesPerRow: width * 4, from: MTLRegionMake2D(0, 0, width, height), mipmapLevel: 0)
        let info = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue
        guard let context = CGContext(
            data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: info) else { return nil }
        return context.makeImage()
    }

    private func encode(
        history: History, into texture: MTLTexture, size: CGSize, scale: Float, scrollY: Float, animate: Bool,
        commands: MTLCommandBuffer
    ) {
        let rowHeight = metrics.rowHeight
        let first = max(0, Int(scrollY / rowHeight) - 1)
        let last = min(history.rowCount, Int((scrollY + Float(size.height)) / rowHeight) + 2)
        instances.removeAll(keepingCapacity: true)
        if first < last {
            updateLanes(history: history, rows: first..<last, width: Float(size.width), animate: animate)
            buildInstances(history: history, rows: first..<last, scrollY: scrollY)
        }

        var uniforms = Uniforms(viewport: [Float(size.width), Float(size.height)], scale: scale)
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)
        pass.colorAttachments[0].storeAction = .store
        guard let encoder = commands.makeRenderCommandEncoder(descriptor: pass) else { return }
        if !instances.isEmpty {
            let buffer = instanceBuffer(count: instances.count)
            instances.withUnsafeBytes { buffer.contents().copyMemory(from: $0.baseAddress!, byteCount: $0.count) }
            encoder.setRenderPipelineState(pipeline)
            encoder.setVertexBuffer(buffer, offset: 0, index: 0)
            encoder.setVertexBytes(&uniforms, length: MemoryLayout<Uniforms>.stride, index: 1)
            encoder.setFragmentBuffer(buffer, offset: 0, index: 0)
            encoder.setFragmentBytes(&uniforms, length: MemoryLayout<Uniforms>.stride, index: 1)
            palette.withUnsafeBytes {
                encoder.setFragmentBytes($0.baseAddress!, length: $0.count, index: 2)
            }
            var paletteCount = UInt32(palette.count)
            encoder.setFragmentBytes(&paletteCount, length: 4, index: 3)
            encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: instances.count)
        }
        encoder.endEncoding()
    }

    /// レーンの今の幅（隣のレーンとの間隔）。
    private func laneSpacing(_ lane: UInt16) -> Float {
        let i = Int(lane)
        guard i < currentX.count else { return metrics.laneWidth }
        let left = i > 0 ? currentX[i] - currentX[i - 1] : metrics.laneWidth
        let right = i + 1 < currentX.count ? currentX[i + 1] - currentX[i] : metrics.laneWidth
        return min(left, right)
    }

    private func buildInstances(history: History, rows: Range<Int>, scrollY: Float) {
        let m = metrics
        let edgeIndices = history.edgeIndices(in: rows)
        history.withEdges { edges in
            for i in edgeIndices {
                let e = edges[Int(i)]
                let a = SIMD2(laneX(e.lane_start), rowY(e.row_start, scrollY: scrollY))
                let b = SIMD2(laneX(e.lane_end), rowY(e.row_end, scrollY: scrollY))
                let firstParent = e.kind & UInt16(AmonhenEdge_FIRST_PARENT) != 0
                var width = firstParent ? m.firstParentWidth : m.lineWidth
                var alpha: Float = 1
                // 詰めたレーンの線は、間隔に合わせて細く薄くし、点を隠さない帯にする。
                let spacing = max(laneSpacing(e.lane_start), laneSpacing(e.lane_end))
                if spacing < m.laneWidth / 2 {
                    width = min(width, max(0.6, spacing * 0.8))
                    alpha = 0.45
                }
                instances.append(Instance(
                    a: a, b: b, width: width, color: UInt32(e.color),
                    kind: e.lane_start == e.lane_end ? Instance.line : Instance.curve, alpha: alpha))
            }
        }
        history.withNodes { nodes in
            for n in nodes[rows] {
                let p = SIMD2(laneX(n.lane), rowY(n.row, scrollY: scrollY))
                instances.append(Instance(a: p, b: p, width: m.nodeRadius, color: UInt32(n.color), kind: Instance.circle))
            }
        }
    }

    /// 3 つを順に使い回す（GPU が前のフレームを読んでいる間に書き換えないため）。
    private func instanceBuffer(count: Int) -> MTLBuffer {
        let length = max(count, 256) * MemoryLayout<Instance>.stride
        if buffers.count < 3 {
            buffers.append(device.makeBuffer(length: length, options: .storageModeShared)!)
        }
        bufferIndex = (bufferIndex + 1) % buffers.count
        if buffers[bufferIndex].length < length {
            buffers[bufferIndex] = device.makeBuffer(length: length * 2, options: .storageModeShared)!
        }
        return buffers[bufferIndex]
    }

    private static let shader = """
    #include <metal_stdlib>
    using namespace metal;

    struct Instance { float2 a; float2 b; float width; uint color; uint kind; float alpha; };
    struct Uniforms { float2 viewport; float scale; float pad; };
    struct VOut { float4 position [[position]]; float2 p; uint instance [[flat]]; };

    vertex VOut graph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                             const device Instance *instances [[buffer(0)]],
                             constant Uniforms &u [[buffer(1)]]) {
        Instance i = instances[iid];
        float pad = i.width + 2.0;
        float2 lo = min(i.a, i.b) - pad;
        float2 hi = max(i.a, i.b) + pad;
        float2 corner = float2((vid & 1) ? hi.x : lo.x, (vid & 2) ? hi.y : lo.y);
        float2 ndc = corner / u.viewport * 2.0 - 1.0;
        VOut o;
        o.position = float4(ndc.x, -ndc.y, 0, 1);
        o.p = corner;
        o.instance = iid;
        return o;
    }

    static float segment(float2 p, float2 a, float2 b) {
        float2 pa = p - a, ba = b - a;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-4), 0.0, 1.0);
        return length(pa - ba * h);
    }

    fragment float4 graph_fragment(VOut in [[stage_in]],
                                   const device Instance *instances [[buffer(0)]],
                                   constant Uniforms &u [[buffer(1)]],
                                   constant float4 *palette [[buffer(2)]],
                                   constant uint &paletteCount [[buffer(3)]]) {
        Instance i = instances[in.instance];
        float d;
        if (i.kind == 1) {
            d = length(in.p - i.a) - i.width;
        } else if (i.kind == 0) {
            d = segment(in.p, i.a, i.b) - i.width * 0.5;
        } else {
            // 縦向きの接線を持つ 3 次ベジェ曲線を、折れ線で近似した距離。
            float my = (i.a.y + i.b.y) * 0.5;
            float2 p1 = float2(i.a.x, my), p2 = float2(i.b.x, my);
            float2 prev = i.a;
            d = 1e9;
            for (int k = 1; k <= 12; k++) {
                float t = float(k) / 12.0, s = 1.0 - t;
                float2 q = s * s * s * i.a + 3.0 * s * s * t * p1 + 3.0 * s * t * t * p2 + t * t * t * i.b;
                d = min(d, segment(in.p, prev, q));
                prev = q;
            }
            d -= i.width * 0.5;
        }
        float alpha = clamp(0.5 - d * u.scale, 0.0, 1.0) * i.alpha;
        float4 c = palette[i.color % paletteCount];
        return float4(c.rgb * alpha, alpha);
    }
    """
}
