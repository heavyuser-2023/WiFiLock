// 메뉴 막대·알림 영역 아이콘을 그린다 (맥에서 실행: swift scripts/make_tray_icons.swift app/icons/tray)
//
// - mac-*.png  : 맥 메뉴 막대용 템플릿 그림 (검은색 + 투명, 다크 모드에서 자동으로 흰색이 됨) 44×44
// - color-*.png: 윈도우·리눅스용 컬러 그림 (밝은·어두운 작업 표시줄 모두에서 보이도록 색 바탕) 64×64
//
// 모양: 모니터 + 받침대 + 화면 안 자물쇠. 잠금이 쉬는 상태는 자물쇠 고리가 열린 모양.
// 다른 앱이 화면을 켜 두어 잠금을 보류한 상태(held)는 화면을 채워 "켜진 화면"으로 그리고, 열린 자물쇠를 뚫어 둔다.

import AppKit
import CoreGraphics
import Foundation

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "app/icons/tray"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

/// 100×100 좌표계(아래가 0)에서 그림을 그린다.
/// lit 이면 화면을 채우고 자물쇠를 구멍으로 낸다. hole 이 nil 이면 투명하게 뚫고(맥 템플릿), 있으면 그 색으로 칠한다(컬러 바탕).
func drawGlyph(_ ctx: CGContext, color: CGColor, open: Bool, stroke: CGFloat, lit: Bool = false, hole: CGColor? = nil) {
    ctx.setStrokeColor(color)
    ctx.setFillColor(color)
    ctx.setLineWidth(stroke)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)

    // 모니터 테두리 (켜진 화면이면 테두리 바깥선까지 채운다)
    let screen = CGRect(x: 8, y: 30, width: 84, height: 58)
    if lit {
        let outer = screen.insetBy(dx: -stroke / 2, dy: -stroke / 2)
        ctx.addPath(CGPath(roundedRect: outer, cornerWidth: 9 + stroke / 2, cornerHeight: 9 + stroke / 2, transform: nil))
        ctx.fillPath()
    } else {
        ctx.addPath(CGPath(roundedRect: screen, cornerWidth: 9, cornerHeight: 9, transform: nil))
        ctx.strokePath()
    }
    // 받침대
    ctx.fill(CGRect(x: 44, y: 16, width: 12, height: 14))
    ctx.addPath(CGPath(roundedRect: CGRect(x: 28, y: 8, width: 44, height: 9), cornerWidth: 4.5, cornerHeight: 4.5, transform: nil))
    ctx.fillPath()

    if lit {
        if let hole {
            ctx.setStrokeColor(hole)
            ctx.setFillColor(hole)
        } else {
            ctx.setBlendMode(.clear)
        }
    }
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
    ctx.setBlendMode(.normal)
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
for (name, open, lit) in [("mac-active.png", false, false), ("mac-inactive.png", true, false), ("mac-held.png", true, true)] {
    let img = render(size: 44) { ctx in
        ctx.translateBy(x: 2, y: 2)
        ctx.scaleBy(x: 0.40, y: 0.40)
        drawGlyph(ctx, color: CGColor(gray: 0, alpha: 1), open: open, stroke: 8, lit: lit)
    }
    save(img, name)
}

// 컬러 (64×64): 둥근 사각형 바탕 + 흰 그림
let colors: [(String, CGColor, Bool, Bool)] = [
    ("color-active.png", CGColor(srgbRed: 0.18, green: 0.44, blue: 0.93, alpha: 1), false, false),
    ("color-inactive.png", CGColor(srgbRed: 0.47, green: 0.50, blue: 0.55, alpha: 1), true, false),
    ("color-warning.png", CGColor(srgbRed: 0.94, green: 0.50, blue: 0.0, alpha: 1), false, false),
    // 잠금 보류: 파란(사용 중)·회색(꺼짐)·주황(경고)과 작게 보여도 구분되는 청록
    ("color-held.png", CGColor(srgbRed: 0.0, green: 0.56, blue: 0.53, alpha: 1), true, true),
]
for (name, bg, open, lit) in colors {
    let img = render(size: 64) { ctx in
        ctx.setFillColor(bg)
        ctx.addPath(CGPath(roundedRect: CGRect(x: 1, y: 1, width: 62, height: 62), cornerWidth: 14, cornerHeight: 14, transform: nil))
        ctx.fillPath()
        ctx.translateBy(x: 8, y: 8)
        ctx.scaleBy(x: 0.48, y: 0.48)
        drawGlyph(ctx, color: CGColor(gray: 1, alpha: 1), open: open, stroke: 8.5, lit: lit, hole: bg)
    }
    save(img, name)
}
