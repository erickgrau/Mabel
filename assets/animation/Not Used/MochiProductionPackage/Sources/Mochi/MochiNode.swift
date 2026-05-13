import SpriteKit
import AppKit

final class MochiNode: SKSpriteNode {
    enum State { case idle, walking, looking, shivering }

    private var state: State = .idle
    private var walkFrames: [SKTexture] = []
    private var facingRight = true

    private let breathingKey = "mochi.breathing"
    private let walkAnimationKey = "mochi.walk.animation"
    private let walkMoveKey = "mochi.walk.move"
    private let earTwitchKey = "mochi.earTwitch"
    private let shiverKey = "mochi.shiver"

    static func make(textureName: String = "mochi_walk_16") -> MochiNode {
        let node = MochiNode(texture: nil, color: .clear, size: CGSize(width: 160, height: 160))
        node.loadWalkTextures(sheetName: textureName)
        node.texture = node.walkFrames.first
        node.anchorPoint = CGPoint(x: 0.5, y: 0.0)
        node.zPosition = 100
        return node
    }

    private func loadWalkTextures(sheetName: String) {
        let sheet = SKTexture(imageNamed: sheetName)
        let frameWidth = CGFloat(1.0 / 16.0)
        walkFrames = (0..<16).map { i in
            let t = SKTexture(rect: CGRect(x: CGFloat(i) * frameWidth, y: 0, width: frameWidth, height: 1.0), in: sheet)
            t.filteringMode = .linear
            return t
        }
    }

    func enterIdle() {
        state = .idle
        removeAction(forKey: walkAnimationKey)
        removeAction(forKey: walkMoveKey)
        removeAction(forKey: shiverKey)
        texture = walkFrames.first
        startBreathing()
        scheduleEarTwitch()

        run(.sequence([
            .wait(forDuration: TimeInterval.random(in: 2.0...5.0)),
            .run { [weak self] in
                guard let self else { return }
                let roll = Int.random(in: 1...100)
                if roll <= 70 { self.enterWalking() }
                else if roll <= 90 { self.enterLooking() }
                else { self.enterIdle() }
            }
        ]))
    }

    func enterWalking() {
        state = .walking
        removeAction(forKey: shiverKey)
        faceRight(Bool.random())

        let frameActions = walkFrames.enumerated().flatMap { index, texture -> [SKAction] in
            var actions: [SKAction] = [.setTexture(texture), .wait(forDuration: 0.125)]
            if [3, 7, 11, 15].contains(index) { actions.append(.wait(forDuration: 0.08)) }
            return actions
        }

        let distance = CGFloat.random(in: 120...340) * (facingRight ? 1 : -1)
        let duration = TimeInterval(abs(distance) / 28.0)

        run(.repeatForever(.sequence(frameActions)), withKey: walkAnimationKey)
        run(.sequence([
            .moveBy(x: distance, y: 0, duration: duration),
            .run { [weak self] in self?.enterIdle() }
        ]), withKey: walkMoveKey)
    }

    func enterLooking() {
        state = .looking
        removeAction(forKey: walkAnimationKey)
        removeAction(forKey: walkMoveKey)
        let look = SKAction.sequence([
            .rotate(byAngle: 0.025, duration: 0.2),
            .wait(forDuration: TimeInterval.random(in: 0.6...1.2)),
            .rotate(byAngle: -0.025, duration: 0.2),
            .run { [weak self] in self?.enterIdle() }
        ])
        look.timingMode = .easeInEaseOut
        run(look)
    }

    func enterRainShiver() {
        state = .shivering
        removeAction(forKey: walkAnimationKey)
        removeAction(forKey: walkMoveKey)

        let shiver = SKAction.repeat(.sequence([
            .moveBy(x: -1.5, y: 0, duration: 0.04),
            .moveBy(x: 3.0, y: 0, duration: 0.08),
            .moveBy(x: -1.5, y: 0, duration: 0.04)
        ]), count: 12)

        run(.sequence([shiver, .wait(forDuration: 0.4), .run { [weak self] in self?.enterIdle() }]), withKey: shiverKey)
    }

    func reactToCursor(at point: CGPoint) {
        guard state != .shivering else { return }
        if hypot(point.x - position.x, point.y - position.y) < 150, state != .looking {
            enterLooking()
        }
    }

    private func faceRight(_ right: Bool) {
        facingRight = right
        xScale = right ? abs(xScale) : -abs(xScale)
    }

    private func startBreathing() {
        removeAction(forKey: breathingKey)
        let inhale = SKAction.scaleY(to: 1.015, duration: 1.4)
        let exhale = SKAction.scaleY(to: 1.0, duration: 1.4)
        inhale.timingMode = .easeInEaseOut
        exhale.timingMode = .easeInEaseOut
        run(.repeatForever(.sequence([inhale, exhale])), withKey: breathingKey)
    }

    private func scheduleEarTwitch() {
        removeAction(forKey: earTwitchKey)
        run(.sequence([
            .wait(forDuration: TimeInterval.random(in: 8...22)),
            .rotate(byAngle: 0.012, duration: 0.05),
            .rotate(byAngle: -0.024, duration: 0.08),
            .rotate(byAngle: 0.012, duration: 0.05),
            .run { [weak self] in self?.scheduleEarTwitch() }
        ]), withKey: earTwitchKey)
    }
}
