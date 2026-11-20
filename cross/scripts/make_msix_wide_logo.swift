// MSIX 와이드 타일 아이콘(310x150 RGBA PNG)을 생성한다.
// 사용법: swift cross/scripts/make_msix_wide_logo.swift <원본_1024.png> <출력.png>
import AppKit

let srcPath = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "cross/icon-src/app-icon-1024.png"
let outPath = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "cross/msix/Assets/Wide310x150Logo.png"

guard let srcImage = NSImage(contentsOfFile: srcPath) else {
    fputs("원본 아이콘을 열 수 없습니다: \(srcPath)\n", stderr)
    exit(1)
}

let W = 310
let H = 150
let iconSize: CGFloat = 126

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: W,
    pixelsHigh: H,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
)!
rep.size = NSSize(width: W, height: H)

NSGraphicsContext.saveGraphicsState()
let ctx = NSGraphicsContext(bitmapImageRep: rep)!
ctx.imageInterpolation = .high
NSGraphicsContext.current = ctx

NSColor.clear.setFill()
NSRect(x: 0, y: 0, width: W, height: H).fill()

let destRect = NSRect(
    x: (CGFloat(W) - iconSize) / 2.0,
    y: (CGFloat(H) - iconSize) / 2.0,
    width: iconSize,
    height: iconSize
)
srcImage.draw(in: destRect, from: .zero, operation: .sourceOver, fraction: 1.0)

NSGraphicsContext.restoreGraphicsState()

let data = rep.representation(using: .png, properties: [:])!
let outURL = URL(fileURLWithPath: outPath)
try FileManager.default.createDirectory(at: outURL.deletingLastPathComponent(), withIntermediateDirectories: true)
try data.write(to: outURL)
print("와이드 로고 저장: \(outPath)")
