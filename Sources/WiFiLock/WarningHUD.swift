import AppKit

/// 잠금 직전 화면 위쪽 가운데에 뜨는 경고 창
final class WarningHUD {
    private var panel: NSPanel?
    private let titleLabel = NSTextField(labelWithString: "")
    private let detailLabel = NSTextField(labelWithString: "마우스나 키보드를 움직이면 취소됩니다")
    private(set) var isVisible = false

    func show(remaining: TimeInterval) {
        let secs = max(1, Int(remaining.rounded(.up)))
        titleLabel.stringValue = "\(secs)초 후 화면이 잠깁니다"
        if panel == nil { build() }
        guard let panel else { return }
        if !isVisible {
            position(panel)
            panel.alphaValue = 0
            panel.orderFrontRegardless()
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.2
                panel.animator().alphaValue = 1
            }
            isVisible = true
        }
    }

    func hide() {
        guard isVisible, let panel else { return }
        isVisible = false
        NSAnimationContext.runAnimationGroup({ ctx in
            ctx.duration = 0.15
            panel.animator().alphaValue = 0
        }, completionHandler: { [weak self] in
            if self?.isVisible == false { panel.orderOut(nil) }
        })
    }

    /// 자가 진단에서 그림으로 남기기 위한 뷰
    var contentViewForSnapshot: NSView? {
        if panel == nil { build() }
        return panel?.contentView
    }

    private func build() {
        let size = NSSize(width: 360, height: 76)
        let p = NSPanel(contentRect: NSRect(origin: .zero, size: size),
                        styleMask: [.borderless, .nonactivatingPanel],
                        backing: .buffered, defer: false)
        p.level = .screenSaver
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = true
        p.ignoresMouseEvents = true
        p.isReleasedWhenClosed = false
        p.hidesOnDeactivate = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]

        let blur = NSVisualEffectView(frame: NSRect(origin: .zero, size: size))
        blur.material = .hudWindow
        blur.blendingMode = .behindWindow
        blur.state = .active
        blur.wantsLayer = true
        blur.layer?.cornerRadius = 16
        blur.layer?.masksToBounds = true

        let icon = NSImageView()
        icon.image = NSImage(systemSymbolName: "lock.display", accessibilityDescription: "화면 잠금")?
            .withSymbolConfiguration(.init(pointSize: 28, weight: .medium))
        icon.contentTintColor = .systemOrange
        icon.translatesAutoresizingMaskIntoConstraints = false

        titleLabel.font = .systemFont(ofSize: 17, weight: .semibold)
        titleLabel.textColor = .labelColor
        detailLabel.font = .systemFont(ofSize: 12)
        detailLabel.textColor = .secondaryLabelColor
        let stack = NSStackView(views: [titleLabel, detailLabel])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 3
        stack.translatesAutoresizingMaskIntoConstraints = false

        blur.addSubview(icon)
        blur.addSubview(stack)
        NSLayoutConstraint.activate([
            icon.leadingAnchor.constraint(equalTo: blur.leadingAnchor, constant: 20),
            icon.centerYAnchor.constraint(equalTo: blur.centerYAnchor),
            icon.widthAnchor.constraint(equalToConstant: 40),
            stack.leadingAnchor.constraint(equalTo: icon.trailingAnchor, constant: 12),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: blur.trailingAnchor, constant: -16),
            stack.centerYAnchor.constraint(equalTo: blur.centerYAnchor),
        ])
        p.contentView = blur
        panel = p
    }

    private func position(_ panel: NSPanel) {
        // 마우스가 있는 화면 위쪽 가운데
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main
        guard let frame = screen?.visibleFrame else { return }
        let size = panel.frame.size
        panel.setFrameOrigin(NSPoint(x: frame.midX - size.width / 2, y: frame.maxY - size.height - 24))
    }
}
