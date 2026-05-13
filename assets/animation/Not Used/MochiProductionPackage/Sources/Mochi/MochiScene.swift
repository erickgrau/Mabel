import SpriteKit
import AppKit

final class MochiScene: SKScene {
    private var mochi: MochiNode!

    override func didMove(to view: SKView) {
        backgroundColor = .clear
        view.allowsTransparency = true
        view.window?.acceptsMouseMovedEvents = true

        mochi = MochiNode.make(textureName: "mochi_walk_16")
        mochi.position = CGPoint(x: size.width / 2, y: 24)
        addChild(mochi)

        mochi.enterIdle()
    }

    override func mouseMoved(with event: NSEvent) {
        mochi.reactToCursor(at: event.location(in: self))
    }

    func triggerRainMode() {
        mochi.enterRainShiver()
    }
}
