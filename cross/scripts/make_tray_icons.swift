// 메뉴 막대·알림 영역 아이콘을 그린다 (맥에서 실행: swift scripts/make_tray_icons.swift app/icons/tray)
//
// - mac-*.png  : 맥 메뉴 막대용 템플릿 그림 (검은색 + 투명, 다크 모드에서 자동으로 흰색이 됨) 44×44
// - color-*.png: 윈도우·리눅스용 컬러 그림 (밝은·어두운 작업 표시줄 모두에서 보이도록 색 바탕) 64×64
//
// 모양: 모니터 + 받침대 + 화면 안 자물쇠. 잠금이 쉬는 상태는 자물쇠 고리가 열린 모양.

import AppKit
import CoreGraphics
import Foundation

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "app/icons/tray"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

/// 100×100 좌표계(아래가 0)에서 그림을 그린다.
func drawGlyph(_ ctx: CGContext, color: CGColor, open: Bool, stroke: CGFloat) {
    ctx.setStrokeColor(color)
    ctx.setFillColor(color)
    ctx.setLineWidth(stroke)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)

    // 모니터 테두리
    let screen = CGRect(x: 8, y: 30, width: 84, height: 58)
    ctx.addPath(CGPath(roundedRect: screen, cornerWidth: 9, cornerHeight: 9, transform: nil))
    ctx.strokePath()
    // 받침대
    ctx.fill(CGRect(x: 44, y: 16, width: 12, height: 14))
    ctx.addPath(CGPath(roundedRect: CGRect(x: 28, y: 8, width: 44, height: 9), cornerWidth: 4.5, cornerHeight: 4.5, transform: nil))
    ctx.fillPath()

    // 자물쇠 몸통
    let body = CGRect(x: 36, y: 40, width: 28, height: 21)
    ctx.addPath(CGPath(roundedRect: body, cornerWidth: 4, cornerHeight: 4, transform: nil))
    ctx.fillPath()
    // 자물쇠 고리
    ctx.setLineWidth(stroke * 0.8)
    let r: CGFloat = 8.5
    if open {
        // 왼쪽 다리는 몸통에 꽂혀 있고, 오른쪽 다리는 위로 떠 있다
        ctx.move(to: CGPoint(x: 50 - r, y: 60))
        ctx.addLine(to: CGPoint(x: 50 - r, y: 71))
        ctx.addArc(center: CGPoint(x: 50, y: 71), radius: r, startAngle: .pi, endAngle: 0, clockwise: true)
        ctx.addLine(to: CGPoint(x: 50 + r, y: 67))
    } else {
        ctx.move(to: CGPoint(x: 50 - r, y: 60))
        ctx.addLine(to: CGPoint(x: 50 - r, y: 66))
        ctx.addArc(center: CGPoint(x: 50, y: 66), radius: r, startAngle: .pi, endAngle: 0, clockwise: true)
        ctx.addLine(to: CGPoint(x: 50 + r, y: 60))
    }
    ctx.strokePath()
}

func render(size: Int, draw: (CGContext) -> Void) -> CGImage {
    let cs = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.setShouldAntialias(true)
    ctx.interpolationQuality = .high
    draw(ctx)
    return ctx.makeImage()!
}

func save(_ image: CGImage, _ name: String) {
    let url = URL(fileURLWithPath: outDir).appendingPathComponent(name)
    let rep = NSBitmapImageRep(cgImage: image)
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
    print("저장: \(url.path)")
}

// 맥 템플릿 (44×44, 여백 2px)
for (name, open) in [("mac-active.png", false), ("mac-inactive.png", true)] {
    let img = render(size: 44) { ctx in
        ctx.translateBy(x: 2, y: 2)
        ctx.scaleBy(x: 0.40, y: 0.40)
        drawGlyph(ctx, color: CGColor(gray: 0, alpha: 1), open: open, stroke: 8)
    }
    save(img, name)
}

// 컬러 (64×64): 둥근 사각형 바탕 + 흰 그림
let colors: [(String, CGColor, Bool)] = [
    ("color-active.png", CGColor(srgbRed: 0.18, green: 0.44, blue: 0.93, alpha: 1), false),
    ("color-inactive.png", CGColor(srgbRed: 0.47, green: 0.50, blue: 0.55, alpha: 1), true),
    ("color-warning.png", CGColor(srgbRed: 0.94, green: 0.50, blue: 0.0, alpha: 1), false),
]
for (name, bg, open) in colors {
    let img = render(size: 64) { ctx in
        ctx.setFillColor(bg)
        ctx.addPath(CGPath(roundedRect: CGRect(x: 1, y: 1, width: 62, height: 62), cornerWidth: 14, cornerHeight: 14, transform: nil))
        ctx.fillPath()
        ctx.translateBy(x: 8, y: 8)
        ctx.scaleBy(x: 0.48, y: 0.48)
        drawGlyph(ctx, color: CGColor(gray: 1, alpha: 1), open: open, stroke: 8.5)
    }
    save(img, name)
}
