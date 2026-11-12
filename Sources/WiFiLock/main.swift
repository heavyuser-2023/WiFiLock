import AppKit

let options = LaunchOptions.parse(CommandLine.arguments)
let app = NSApplication.shared
let delegate = AppDelegate(options: options)
app.delegate = delegate
// 독(Dock)에 나타나지 않는 메뉴 막대 전용 앱
app.setActivationPolicy(.accessory)
app.run()
