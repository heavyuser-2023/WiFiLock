// 앱 아이콘(1024px PNG)을 직접 그린다: 파란 배경 위에 모니터, 화면 안에 자물쇠, 위쪽에 와이파이 물결.
// 사용법: swift scripts/make_icon.swift <출력.png>
import AppKit

let outPath = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon_1024.png"
let S: CGFloat = 1024

let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(S), pixelsHigh: Int(S), bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                           bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
let ctx = NSGraphicsContext(bitmapImageRep: rep)!
NSGraphicsContext.current = ctx
let cg = ctx.cgContext

func color(_ hex: UInt32, _ a: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

// 1) 배경: macOS 아이콘 격자(824px 둥근 사각형)
let bgRect = NSRect(x: 100, y: 100, width: 824, height: 824)
let bgPath = NSBezierPath(roundedRect: bgRect, xRadius: 185, yRadius: 185)
cg.saveGState()
cg.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: NSColor.black.withAlphaComponent(0.35).cgColor)
color(0x1E3A8A).setFill()
bgPath.fill()
cg.restoreGState()
NSGradient(colors: [color(0x3B82F6), color(0x4338CA)])!.draw(in: bgPath, angle: -90)
// 위쪽 은은한 광택
NSGradient(colorsAndLocations: (NSColor.white.withAlphaComponent(0.16), 0.0), (NSColor.white.withAlphaComponent(0), 0.55))!
    .draw(in: bgPath, angle: -90)

// 2) 와이파이 물결 (위쪽)
let wifiCenter = NSPoint(x: 512, y: 610)
NSColor.white.withAlphaComponent(0.9).setStroke()
for (i, r) in [CGFloat(250), 185, 120].enumerated() {
    let arc = NSBezierPath()
    arc.lineWidth = 34
    arc.lineCapStyle = .round
    arc.appendArc(withCenter: wifiCenter, radius: r, startAngle: 50, endAngle: 130)
    NSColor.white.withAlphaComponent(0.55 + CGFloat(i) * 0.15).setStroke()
    arc.stroke()
}

// 3) 모니터
let screenRect = NSRect(x: 252, y: 300, width: 520, height: 330)
cg.saveGState()
cg.setShadow(offset: CGSize(width: 0, height: -10), blur: 24, color: NSColor.black.withAlphaComponent(0.3).cgColor)
NSColor.white.setFill()
NSBezierPath(roundedRect: screenRect, xRadius: 42, yRadius: 42).fill()
cg.restoreGState()
// 화면 안쪽
let inner = screenRect.insetBy(dx: 26, dy: 26)
NSGradient(colors: [color(0x0F172A), color(0x1E293B)])!.draw(in: NSBezierPath(roundedRect: inner, xRadius: 22, yRadius: 22), angle: -90)
// 받침대
NSColor.white.setFill()
NSBezierPath(rect: NSRect(x: 472, y: 238, width: 80, height: 64)).fill()
NSBezierPath(roundedRect: NSRect(x: 382, y: 214, width: 260, height: 34), xRadius: 17, yRadius: 17).fill()

// 4) 자물쇠 (화면 가운데)
let lockBody = NSRect(x: 437, y: 352, width: 150, height: 118)
let shackle = NSBezierPath()
shackle.lineWidth = 26
shackle.lineCapStyle = .round
shackle.move(to: NSPoint(x: 466, y: 470))
shackle.line(to: NSPoint(x: 466, y: 505))
shackle.appendArc(withCenter: NSPoint(x: 512, y: 505), radius: 46, startAngle: 180, endAngle: 0, clockwise: true)
shackle.line(to: NSPoint(x: 558, y: 470))
color(0xFBBF24).setStroke()
shackle.stroke()
NSGradient(colors: [color(0xFCD34D), color(0xF59E0B)])!.draw(in: NSBezierPath(roundedRect: lockBody, xRadius: 22, yRadius: 22), angle: -90)
// 열쇠 구멍
color(0x78350F).setFill()
NSBezierPath(ovalIn: NSRect(x: 497, y: 413, width: 30, height: 30)).fill()
NSBezierPath(roundedRect: NSRect(x: 505, y: 380, width: 14, height: 42), xRadius: 7, yRadius: 7).fill()

NSGraphicsContext.restoreGraphicsState()
let data = rep.representation(using: .png, properties: [:])!
try! data.write(to: URL(fileURLWithPath: outPath))
print("아이콘 저장: \(outPath)")
