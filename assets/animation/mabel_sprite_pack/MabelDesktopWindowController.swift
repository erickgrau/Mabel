import AppKit
import SpriteKit

final class MabelDesktopWindowController: NSWindowController {

    private let scene = MabelScene(size: NSScreen.main?.frame.size ?? CGSize(width: 1200, height: 800))

    convenience init() {
        let screenFrame = NSScreen.main?.frame ?? NSRect(x: 0, y: 0, width: 1200, height: 800)

        let window = NSWindow(
            contentRect: screenFrame,
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )

        window.isOpaque = false
        window.backgroundColor = .clear
        window.level = .screenSaver
        window.ignoresMouseEvents = true
        window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]

        let skView = SKView(frame: screenFrame)
        skView.allowsTransparency = true
        skView.presentScene(scene)

        window.contentView = skView
        self.init(window: window)

        NSEvent.addGlobalMonitorForEvents(matching: [.mouseMoved]) { [weak self] event in
            self?.scene.lookAtCursor(screenPoint: NSEvent.mouseLocation, in: window)
        }
    }

    func showCat() {
        window?.makeKeyAndOrderFront(nil)
    }
}
