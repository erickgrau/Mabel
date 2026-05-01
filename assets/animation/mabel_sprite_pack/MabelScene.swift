import SpriteKit
import AppKit

final class MabelScene: SKScene {

    private var cat: SKSpriteNode!
    private var walkFrames: [SKTexture] = []
    private var sitTexture: SKTexture!
    private var blinkTexture: SKTexture!

    private var isFacingRight = true
    private let catDisplaySize = CGSize(width: 160, height: 160)

    override func didMove(to view: SKView) {
        backgroundColor = .clear
        scaleMode = .resizeFill
        loadTextures()
        setupCat()
        runRoamingLoop()
    }

    private func loadTextures() {
        let sheet = SKTexture(imageNamed: "mabel_sprite_4096x512")
        let frameWidth: CGFloat = 1.0 / 8.0

        for index in 0..<8 {
            let texture = SKTexture(
                rect: CGRect(
                    x: CGFloat(index) * frameWidth,
                    y: 0,
                    width: frameWidth,
                    height: 1.0
                ),
                in: sheet
            )

            texture.filteringMode = .linear

            if index < 6 {
                walkFrames.append(texture)
            } else if index == 6 {
                sitTexture = texture
            } else {
                blinkTexture = texture
            }
        }
    }

    private func setupCat() {
        cat = SKSpriteNode(texture: walkFrames[0])
        cat.size = catDisplaySize
        cat.anchorPoint = CGPoint(x: 0.5, y: 0.0)
        cat.position = CGPoint(x: 120, y: 24)
        cat.zPosition = 10
        addChild(cat)
    }

    private func runRoamingLoop() {
        cat.removeAllActions()

        let decide = SKAction.run { [weak self] in
            guard let self else { return }

            if Bool.random() {
                self.walkRandomDirection()
            } else {
                self.sitAndBlink()
            }
        }

        cat.run(.repeatForever(.sequence([
            decide,
            .wait(forDuration: Double.random(in: 1.0...2.5))
        ])))
    }

    private func walkRandomDirection() {
        cat.removeAllActions()

        let margin: CGFloat = 80
        let minX = margin
        let maxX = max(margin, size.width - margin)
        let targetX = CGFloat.random(in: minX...maxX)

        let goingRight = targetX >= cat.position.x
        setFacingRight(goingRight)

        let distance = abs(targetX - cat.position.x)
        let duration = max(1.5, Double(distance / 70.0))

        let walkAnimation = SKAction.repeatForever(
            .animate(with: walkFrames, timePerFrame: 0.10, resize: false, restore: false)
        )

        let move = SKAction.moveTo(x: targetX, duration: duration)
        move.timingMode = .easeInEaseOut

        cat.run(walkAnimation, withKey: "walk")
        cat.run(.sequence([
            move,
            .run { [weak self] in
                self?.cat.removeAction(forKey: "walk")
                self?.cat.texture = self?.sitTexture
            }
        ]))
    }

    private func sitAndBlink() {
        cat.removeAllActions()
        cat.texture = sitTexture

        let blinkOnce = SKAction.sequence([
            .wait(forDuration: Double.random(in: 1.2...3.5)),
            .setTexture(blinkTexture),
            .wait(forDuration: 0.12),
            .setTexture(sitTexture)
        ])

        cat.run(.repeat(blinkOnce, count: Int.random(in: 1...3)))
    }

    private func setFacingRight(_ right: Bool) {
        guard right != isFacingRight else { return }
        isFacingRight = right

        let newScaleX = right ? abs(cat.xScale) : -abs(cat.xScale)
        cat.run(.scaleX(to: newScaleX, duration: 0.15))
    }

    func lookAtCursor(screenPoint: CGPoint, in window: NSWindow?) {
        guard let window else { return }

        let localPoint = window.convertPoint(fromScreen: screenPoint)
        let scenePoint = CGPoint(x: localPoint.x, y: localPoint.y)

        // Tiny behavior: if cursor is near the cat, stop and face it.
        let dx = scenePoint.x - cat.position.x
        let dy = scenePoint.y - cat.position.y
        let distance = hypot(dx, dy)

        if distance < 180 {
            cat.removeAction(forKey: "walk")
            cat.texture = sitTexture
            setFacingRight(dx >= 0)
        }
    }
}
