// Renders the ReleaseDock app icon: a package on a dock, released upward.
// Writes every macOS size into App/Resources/Assets.xcassets/AppIcon.appiconset
// and a 1024 px preview to docs/images/icon.png.
//
// Usage: swift scripts/generate-icon.swift <repo-root>
import AppKit

let root = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : FileManager.default.currentDirectoryPath)
let iconSet = root.appendingPathComponent("App/Resources/Assets.xcassets/AppIcon.appiconset")
let imagesDir = root.appendingPathComponent("docs/images")
try FileManager.default.createDirectory(at: iconSet, withIntermediateDirectories: true)
try FileManager.default.createDirectory(at: imagesDir, withIntermediateDirectories: true)

func rgb(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
}

func linearGradient(_ ctx: CGContext, _ colors: [CGColor], from: CGPoint, to: CGPoint) {
    let gradient = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: colors as CFArray, locations: nil)!
    ctx.drawLinearGradient(gradient, start: from, end: to, options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
}

func polygon(_ points: [CGPoint]) -> CGPath {
    let path = CGMutablePath()
    path.addLines(between: points)
    path.closeSubpath()
    return path
}

/// Draws on a 1024×1024 canvas (origin bottom-left); `scale` maps to the output size.
func draw(_ ctx: CGContext) {
    // Apple's macOS grid: 824 pt rounded square centred in 1024 with room for the shadow.
    let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
    let tilePath = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)

    // Drop shadow
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -14), blur: 28, color: rgb(0x000000, 0.35))
    ctx.addPath(tilePath)
    ctx.setFillColor(rgb(0x0B2C55))
    ctx.fillPath()
    ctx.restoreGState()

    ctx.saveGState()
    ctx.addPath(tilePath)
    ctx.clip()

    // Sky: deep navy to bright blue
    linearGradient(ctx, [rgb(0x2D8CFF), rgb(0x1556B8), rgb(0x0A2A5E)], from: CGPoint(x: 512, y: 924), to: CGPoint(x: 512, y: 100))

    // Soft glow behind the package
    let glow = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [rgb(0x8FD3FF, 0.55), rgb(0x8FD3FF, 0)] as CFArray, locations: [0, 1])!
    ctx.drawRadialGradient(glow, startCenter: CGPoint(x: 512, y: 600), startRadius: 0, endCenter: CGPoint(x: 512, y: 600), endRadius: 380, options: [])

    // Water
    let waterTop: CGFloat = 300
    ctx.saveGState()
    ctx.clip(to: CGRect(x: 0, y: 0, width: 1024, height: waterTop))
    linearGradient(ctx, [rgb(0x0F6FB8), rgb(0x063A73)], from: CGPoint(x: 512, y: waterTop), to: CGPoint(x: 512, y: 100))
    ctx.restoreGState()
    ctx.setStrokeColor(rgb(0xBFE6FF, 0.55))
    ctx.setLineWidth(14)
    ctx.setLineCap(.round)
    for (y, phase) in [(CGFloat(240), CGFloat(0)), (CGFloat(185), CGFloat(60))] {
        let wave = CGMutablePath()
        wave.move(to: CGPoint(x: 120 + phase, y: y))
        var x = 120 + phase
        while x < 900 {
            wave.addQuadCurve(to: CGPoint(x: x + 60, y: y), control: CGPoint(x: x + 30, y: y + 22))
            wave.addQuadCurve(to: CGPoint(x: x + 120, y: y), control: CGPoint(x: x + 90, y: y - 22))
            x += 120
        }
        ctx.addPath(wave)
        ctx.strokePath()
    }

    // Dock: pylons and deck
    for x in [CGFloat(235), 512, 789] {
        ctx.addPath(CGPath(roundedRect: CGRect(x: x - 26, y: 150, width: 52, height: 200), cornerWidth: 14, cornerHeight: 14, transform: nil))
        ctx.setFillColor(rgb(0x6B3F1D))
        ctx.fillPath()
    }
    let deck = CGRect(x: 170, y: 318, width: 684, height: 58)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -8), blur: 12, color: rgb(0x000000, 0.35))
    ctx.addPath(CGPath(roundedRect: deck, cornerWidth: 18, cornerHeight: 18, transform: nil))
    ctx.setFillColor(rgb(0xB8743A))
    ctx.fillPath()
    ctx.restoreGState()
    ctx.addPath(CGPath(roundedRect: deck, cornerWidth: 18, cornerHeight: 18, transform: nil))
    ctx.clip()
    linearGradient(ctx, [rgb(0xE0A562), rgb(0xA8642D)], from: CGPoint(x: 512, y: deck.maxY), to: CGPoint(x: 512, y: deck.minY))
    ctx.resetClip()
    ctx.addPath(tilePath)
    ctx.clip()
    ctx.setStrokeColor(rgb(0x7A4520, 0.6))
    ctx.setLineWidth(5)
    for x in stride(from: CGFloat(285), to: 854, by: 115) {
        ctx.move(to: CGPoint(x: x, y: deck.minY + 8))
        ctx.addLine(to: CGPoint(x: x, y: deck.maxY - 8))
    }
    ctx.strokePath()

    // Package (isometric cube) resting on the deck
    let w: CGFloat = 170, h: CGFloat = 98, d: CGFloat = 158
    let cy = deck.maxY + h + d - 4      // bottom front vertex sits on the deck
    let back = CGPoint(x: 512, y: cy + h), left = CGPoint(x: 512 - w, y: cy)
    let right = CGPoint(x: 512 + w, y: cy), frontVertex = CGPoint(x: 512, y: cy - h)
    let bottomFront = CGPoint(x: 512, y: frontVertex.y - d)
    let bottomLeft = CGPoint(x: left.x, y: left.y - d), bottomRight = CGPoint(x: right.x, y: right.y - d)

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 24, color: rgb(0x000000, 0.4))
    ctx.addPath(polygon([left, back, right, bottomRight, bottomFront, bottomLeft]))
    ctx.setFillColor(rgb(0xD08F45))
    ctx.fillPath()
    ctx.restoreGState()

    // Left face
    ctx.saveGState()
    ctx.addPath(polygon([left, frontVertex, bottomFront, bottomLeft]))
    ctx.clip()
    linearGradient(ctx, [rgb(0xF2B866), rgb(0xD9923F)], from: left, to: bottomFront)
    ctx.restoreGState()
    // Right face
    ctx.saveGState()
    ctx.addPath(polygon([frontVertex, right, bottomRight, bottomFront]))
    ctx.clip()
    linearGradient(ctx, [rgb(0xC98235), rgb(0xA9661F)], from: right, to: bottomFront)
    ctx.restoreGState()
    // Top face
    ctx.saveGState()
    ctx.addPath(polygon([left, back, right, frontVertex]))
    ctx.clip()
    linearGradient(ctx, [rgb(0xFFE1A8), rgb(0xF6C47C)], from: back, to: frontVertex)
    ctx.restoreGState()

    // Tape across the top and down the front edges
    let tape: CGFloat = 0.14
    func lerp(_ a: CGPoint, _ b: CGPoint, _ t: CGFloat) -> CGPoint { CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t) }
    ctx.addPath(polygon([lerp(left, back, 0.5 - tape), lerp(left, back, 0.5 + tape), lerp(frontVertex, right, 0.5 + tape), lerp(frontVertex, right, 0.5 - tape)]))
    ctx.setFillColor(rgb(0xFFF3DC, 0.85))
    ctx.fillPath()
    ctx.addPath(polygon([lerp(frontVertex, right, 0.5 - tape), lerp(frontVertex, right, 0.5 + tape), lerp(bottomFront, bottomRight, 0.5 + tape), lerp(bottomFront, bottomRight, 0.5 - tape)]))
    ctx.setFillColor(rgb(0xF4DDB4, 0.75))
    ctx.fillPath()

    // Edge highlights
    ctx.setStrokeColor(rgb(0xFFFFFF, 0.35))
    ctx.setLineWidth(4)
    ctx.setLineJoin(.round)
    ctx.addPath(polygon([left, back, right, frontVertex]))
    ctx.strokePath()

    // Release arrow rising from the package
    let arrow = CGMutablePath()
    let ax: CGFloat = 512, shaftBottom: CGFloat = back.y + 28, shaftTop: CGFloat = back.y + 92, headTop: CGFloat = back.y + 172
    arrow.move(to: CGPoint(x: ax - 30, y: shaftBottom))
    arrow.addLine(to: CGPoint(x: ax - 30, y: shaftTop))
    arrow.addLine(to: CGPoint(x: ax - 84, y: shaftTop))
    arrow.addLine(to: CGPoint(x: ax, y: headTop))
    arrow.addLine(to: CGPoint(x: ax + 84, y: shaftTop))
    arrow.addLine(to: CGPoint(x: ax + 30, y: shaftTop))
    arrow.addLine(to: CGPoint(x: ax + 30, y: shaftBottom))
    arrow.closeSubpath()
    ctx.saveGState()
    ctx.setShadow(offset: .zero, blur: 30, color: rgb(0xBDE9FF, 0.9))
    ctx.addPath(arrow)
    ctx.setFillColor(rgb(0xFFFFFF))
    ctx.fillPath()
    ctx.restoreGState()

    // Glossy top sheen
    ctx.addPath(tilePath)
    ctx.clip()
    linearGradient(ctx, [rgb(0xFFFFFF, 0.16), rgb(0xFFFFFF, 0)], from: CGPoint(x: 512, y: 924), to: CGPoint(x: 512, y: 640))
    ctx.restoreGState()

    // Hairline border
    ctx.addPath(tilePath)
    ctx.setStrokeColor(rgb(0xFFFFFF, 0.18))
    ctx.setLineWidth(3)
    ctx.strokePath()
}

func render(size: Int) -> Data {
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    ctx.setShouldAntialias(true)
    ctx.scaleBy(x: CGFloat(size) / 1024, y: CGFloat(size) / 1024)
    draw(ctx)
    let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
    return rep.representation(using: .png, properties: [:])!
}

var images: [[String: String]] = []
for base in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = "icon_\(base)x\(base)\(scale == 2 ? "@2x" : "").png"
        try render(size: base * scale).write(to: iconSet.appendingPathComponent(name))
        images.append(["idiom": "mac", "size": "\(base)x\(base)", "scale": "\(scale)x", "filename": name])
    }
}
let contents: [String: Any] = ["images": images, "info": ["author": "xcode", "version": 1]]
try JSONSerialization.data(withJSONObject: contents, options: [.prettyPrinted, .sortedKeys]).write(to: iconSet.appendingPathComponent("Contents.json"))
try render(size: 1024).write(to: imagesDir.appendingPathComponent("icon.png"))
print("Wrote \(images.count) icon images and docs/images/icon.png")
