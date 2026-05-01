import AppKit
import SpriteKit

public final class MabelDesktopWindowController: NSWindowController {
    public let spriteView: SKView
    public let mabelScene: MabelScene

    public init(
        frame: NSRect = NSRect(x: 80, y: 80, width: 720, height: 260),
        configuration: MabelConfiguration = MabelConfiguration()
    ) {
        spriteView = SKView(frame: frame)
        spriteView.allowsTransparency = true
        spriteView.ignoresSiblingOrder = true
        spriteView.wantsLayer = true
        spriteView.layer?.backgroundColor = NSColor.clear.cgColor

        mabelScene = MabelScene(size: frame.size, configuration: configuration)

        let window = NSWindow(
            contentRect: frame,
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )

        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = false
        window.level = .screenSaver
        window.ignoresMouseEvents = false
        window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        window.contentView = spriteView

        super.init(window: window)
        spriteView.presentScene(mabelScene)
        window.acceptsMouseMovedEvents = true
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    public func show() {
        window?.makeKeyAndOrderFront(nil)
    }

    public func hideMabel() {
        window?.orderOut(nil)
    }
}
