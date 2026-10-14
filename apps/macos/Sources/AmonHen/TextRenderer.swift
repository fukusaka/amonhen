import AppKit
import CoreText
import Metal
import QuartzCore

/// グリフのアトラス。Core Text でラスタライズしたグリフを 1 枚のテクスチャに詰める。
/// 横位置は 1/4 ピクセル単位に丸め、その 4 通りを別のグリフとして持つ。
final class GlyphAtlas {
    struct Entry {
        /// アトラスの中のピクセル位置と大きさ。
        var x: Float, y: Float, width: Float, height: Float
        /// ペン位置（ベースラインの左端）からの、グリフの画像の左上までのずれ（ピクセル）。
        var left: Float, top: Float
        var isColor: Bool
    }

    private struct Key: Hashable {
        let font: String
        let size: CGFloat
        let glyph: CGGlyph
        let subpixel: UInt8
    }

    static let size = 2048
    static let subpixelSteps = 4
    let texture: MTLTexture
    private var entries: [Key: Entry?] = [:]
    private var cursorX = 1, cursorY = 1, shelfHeight = 0

    init?(device: MTLDevice) {
        let desc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm, width: Self.size, height: Self.size, mipmapped: false)
        desc.usage = .shaderRead
        desc.storageMode = .shared
        guard let texture = device.makeTexture(descriptor: desc) else { return nil }
        self.texture = texture
    }

    /// 空白などで描くものがなければ nil。アトラスが一杯になったら空にして詰め直す（呼び出し側は描き直す）。
    func entry(font: CTFont, glyph: CGGlyph, subpixel: Int, scale: CGFloat) -> Entry? {
        let key = Key(font: CTFontCopyPostScriptName(font) as String, size: CTFontGetSize(font), glyph: glyph,
                      subpixel: UInt8(subpixel))
        if let cached = entries[key] { return cached }
        let entry = rasterize(font: font, glyph: glyph, subpixel: subpixel, scale: scale)
        entries[key] = entry
        return entry
    }

    private func rasterize(font: CTFont, glyph: CGGlyph, subpixel: Int, scale: CGFloat) -> Entry? {
        var g = glyph
        var bounds = CGRect.zero
        CTFontGetBoundingRectsForGlyphs(font, .default, &g, &bounds, 1)
        if bounds.isEmpty { return nil }
        let shift = CGFloat(subpixel) / CGFloat(Self.subpixelSteps)
        // ピクセル単位の外接矩形（アンチエイリアスの分 1 ピクセル広げる）。
        let left = Int(floor(bounds.minX * scale + shift)) - 1
        let right = Int(ceil(bounds.maxX * scale + shift)) + 1
        let bottom = Int(floor(bounds.minY * scale)) - 1
        let top = Int(ceil(bounds.maxY * scale)) + 1
        let width = right - left, height = top - bottom
        guard width > 0, height > 0, width < Self.size, height < Self.size else { return nil }

        if cursorX + width + 1 > Self.size {
            cursorX = 1
            cursorY += shelfHeight + 1
            shelfHeight = 0
        }
        if cursorY + height + 1 > Self.size {
            // 一杯になったら作り直す。めったに起きない。
            entries.removeAll()
            cursorX = 1
            cursorY = 1
            shelfHeight = 0
        }

        let isColor = CTFontGetSymbolicTraits(font).contains(.traitColorGlyphs)
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let info = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue
        pixels.withUnsafeMutableBytes { buf in
            guard let ctx = CGContext(
                data: buf.baseAddress, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: info) else { return }
            ctx.setAllowsAntialiasing(true)
            ctx.setShouldAntialias(true)
            ctx.setAllowsFontSubpixelPositioning(true)
            ctx.setShouldSubpixelPositionFonts(true)
            // 単色のグリフは白で描き、色はシェーダーで付ける。
            ctx.setFillColor(CGColor(gray: 1, alpha: 1))
            ctx.scaleBy(x: scale, y: scale)
            var position = CGPoint(x: (CGFloat(-left) + shift) / scale, y: CGFloat(-bottom) / scale)
            CTFontDrawGlyphs(font, &g, &position, 1, ctx)
        }
        texture.replace(
            region: MTLRegionMake2D(cursorX, cursorY, width, height), mipmapLevel: 0, withBytes: pixels,
            bytesPerRow: width * 4)
        let entry = Entry(
            x: Float(cursorX), y: Float(cursorY), width: Float(width), height: Float(height),
            left: Float(left), top: Float(top), isColor: isColor)
        cursorX += width + 1
        shelfHeight = max(shelfHeight, height)
        return entry
    }
}

/// 色付きの四角形とグリフを、四角形のインスタンスとして 1 回の描画命令で描く。
final class TextRenderer {
    struct Instance {
        /// 描く位置と大きさ（ポイント、左上原点）。
        var rect: SIMD4<Float>
        /// アトラスの中の位置と大きさ（ピクセル）。
        var uv: SIMD4<Float>
        var color: SIMD4<Float>
        /// 0: 塗りつぶし、1: 単色のグリフ、2: カラーのグリフ。
        var kind: UInt32
        var pad: SIMD3<UInt32> = .zero
    }

    /// 1 行分の並べたグリフ。行を描くたびに Core Text を呼ばないようにキャッシュする。
    struct ShapedLine {
        struct Glyph {
            var entry: GlyphAtlas.Entry
            /// 行の左端からのペン位置（ピクセル、1/4 ピクセルに丸めた後の整数部）。
            var penX: Float
        }

        var glyphs: [Glyph]
        var width: CGFloat
    }

    let device: MTLDevice
    let atlas: GlyphAtlas
    private let queue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private var buffers: [MTLBuffer] = []
    private var bufferIndex = 0
    var instances: [Instance] = []

    init?(device: MTLDevice) {
        guard let queue = device.makeCommandQueue(), let atlas = GlyphAtlas(device: device) else { return nil }
        self.device = device
        self.queue = queue
        self.atlas = atlas
        do {
            let library = try device.makeLibrary(source: Self.shader, options: nil)
            let desc = MTLRenderPipelineDescriptor()
            desc.vertexFunction = library.makeFunction(name: "text_vertex")
            desc.fragmentFunction = library.makeFunction(name: "text_fragment")
            let color = desc.colorAttachments[0]!
            color.pixelFormat = .bgra8Unorm
            color.isBlendingEnabled = true
            color.sourceRGBBlendFactor = .one
            color.sourceAlphaBlendFactor = .one
            color.destinationRGBBlendFactor = .oneMinusSourceAlpha
            color.destinationAlphaBlendFactor = .oneMinusSourceAlpha
            pipeline = try device.makeRenderPipelineState(descriptor: desc)
        } catch {
            NSLog("文字のパイプラインを作れない: \(error)")
            return nil
        }
    }

    /// `string` を `font` で並べる。フォントにない文字は Core Text が代わりのフォントを選ぶ。
    func shape(_ string: CFString, font: CTFont, scale: CGFloat) -> ShapedLine {
        let attributed = CFAttributedStringCreate(
            nil, string, [kCTFontAttributeName: font] as CFDictionary)!
        let line = CTLineCreateWithAttributedString(attributed)
        var glyphs: [ShapedLine.Glyph] = []
        for run in CTLineGetGlyphRuns(line) as! [CTRun] {
            let count = CTRunGetGlyphCount(run)
            guard count > 0 else { continue }
            let attrs = CTRunGetAttributes(run) as NSDictionary
            let runFont = attrs[kCTFontAttributeName] as! CTFont
            var ids = [CGGlyph](repeating: 0, count: count)
            var positions = [CGPoint](repeating: .zero, count: count)
            CTRunGetGlyphs(run, CFRange(location: 0, length: count), &ids)
            CTRunGetPositions(run, CFRange(location: 0, length: count), &positions)
            for i in 0..<count {
                let x = positions[i].x * scale
                let whole = floor(x)
                let subpixel = Int(((x - whole) * CGFloat(GlyphAtlas.subpixelSteps)).rounded()) % GlyphAtlas.subpixelSteps
                if let entry = atlas.entry(font: runFont, glyph: ids[i], subpixel: subpixel, scale: scale) {
                    glyphs.append(.init(entry: entry, penX: Float(whole)))
                }
            }
        }
        return ShapedLine(glyphs: glyphs, width: CTLineGetTypographicBounds(line, nil, nil, nil))
    }

    func addRect(_ rect: CGRect, color: SIMD4<Float>) {
        instances.append(Instance(
            rect: [Float(rect.minX), Float(rect.minY), Float(rect.width), Float(rect.height)], uv: .zero,
            color: color, kind: 0))
    }

    /// `line` を、ベースラインの左端が `origin`（ポイント）になるように置く。
    func addLine(_ line: ShapedLine, origin: CGPoint, color: SIMD4<Float>, scale: CGFloat) {
        let s = Float(scale)
        let ox = Float((origin.x * scale).rounded()), oy = Float((origin.y * scale).rounded())
        for g in line.glyphs {
            let e = g.entry
            let x = (ox + g.penX + e.left) / s
            let y = (oy - e.top) / s
            instances.append(Instance(
                rect: [x, y, e.width / s, e.height / s], uv: [e.x, e.y, e.width, e.height], color: color,
                kind: e.isColor ? 2 : 1))
        }
    }

    /// 積んだインスタンスを描く。
    func draw(
        layer: CAMetalLayer, background: SIMD4<Float>, onPresented: (@Sendable (CFTimeInterval) -> Void)? = nil
    ) {
        let size = layer.bounds.size
        guard size.width > 0, size.height > 0, let drawable = layer.nextDrawable(),
              let commands = queue.makeCommandBuffer() else { return }
        encode(into: drawable.texture, size: size, background: background, commands: commands)
        if let onPresented {
            drawable.addPresentedHandler { onPresented($0.presentedTime) }
        }
        if layer.presentsWithTransaction {
            commands.commit()
            commands.waitUntilScheduled()
            drawable.present()
        } else {
            commands.present(drawable)
            commands.commit()
        }
    }

    /// 積んだインスタンスを画面に出さずに描いて画像にする（`--snapshot` 用）。
    func image(size: CGSize, scale: CGFloat, background: SIMD4<Float>) -> CGImage? {
        let width = Int(size.width * scale), height = Int(size.height * scale)
        let desc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm, width: max(width, 1), height: max(height, 1), mipmapped: false)
        desc.usage = [.renderTarget, .shaderRead]
        desc.storageMode = .shared
        guard width > 0, height > 0, let texture = device.makeTexture(descriptor: desc),
              let commands = queue.makeCommandBuffer() else { return nil }
        encode(into: texture, size: size, background: background, commands: commands)
        commands.commit()
        commands.waitUntilCompleted()
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        texture.getBytes(&pixels, bytesPerRow: width * 4, from: MTLRegionMake2D(0, 0, width, height), mipmapLevel: 0)
        let info = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue
        return CGContext(
            data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: info)?.makeImage()
    }

    private func encode(into texture: MTLTexture, size: CGSize, background: SIMD4<Float>, commands: MTLCommandBuffer) {
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].clearColor = MTLClearColor(
            red: Double(background.x), green: Double(background.y), blue: Double(background.z),
            alpha: Double(background.w))
        pass.colorAttachments[0].storeAction = .store
        guard let encoder = commands.makeRenderCommandEncoder(descriptor: pass) else { return }
        if !instances.isEmpty {
            let buffer = instanceBuffer(count: instances.count)
            instances.withUnsafeBytes { buffer.contents().copyMemory(from: $0.baseAddress!, byteCount: $0.count) }
            var viewport = SIMD2<Float>(Float(size.width), Float(size.height))
            var atlasSize = Float(GlyphAtlas.size)
            encoder.setRenderPipelineState(pipeline)
            encoder.setVertexBuffer(buffer, offset: 0, index: 0)
            encoder.setVertexBytes(&viewport, length: MemoryLayout<SIMD2<Float>>.stride, index: 1)
            encoder.setVertexBytes(&atlasSize, length: 4, index: 2)
            encoder.setFragmentTexture(atlas.texture, index: 0)
            encoder.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4, instanceCount: instances.count)
        }
        encoder.endEncoding()
    }

    private func instanceBuffer(count: Int) -> MTLBuffer {
        let length = max(count, 1024) * MemoryLayout<Instance>.stride
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

    struct Instance { float4 rect; float4 uv; float4 color; uint kind; uint3 pad; };
    struct VOut { float4 position [[position]]; float2 uv; float4 color; uint kind [[flat]]; };

    vertex VOut text_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                            const device Instance *instances [[buffer(0)]],
                            constant float2 &viewport [[buffer(1)]],
                            constant float &atlasSize [[buffer(2)]]) {
        Instance i = instances[iid];
        float2 corner = float2(vid & 1, (vid >> 1) & 1);
        float2 p = i.rect.xy + corner * i.rect.zw;
        float2 ndc = p / viewport * 2.0 - 1.0;
        VOut o;
        o.position = float4(ndc.x, -ndc.y, 0, 1);
        o.uv = (i.uv.xy + corner * i.uv.zw) / atlasSize;
        o.color = i.color;
        o.kind = i.kind;
        return o;
    }

    fragment float4 text_fragment(VOut in [[stage_in]], texture2d<float> atlas [[texture(0)]]) {
        constexpr sampler s(coord::normalized, filter::nearest);
        if (in.kind == 0) {
            return float4(in.color.rgb * in.color.a, in.color.a);
        }
        float4 t = atlas.sample(s, in.uv);
        if (in.kind == 2) {
            return t;
        }
        float a = t.a * in.color.a;
        return float4(in.color.rgb * a, a);
    }
    """
}
