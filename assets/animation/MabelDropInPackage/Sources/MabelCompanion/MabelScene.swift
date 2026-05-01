import AppKit
import SpriteKit

public final class MabelScene: SKScene {
    public private(set) var state: MabelState = .sitting

    private let config: MabelConfiguration
    private var cat: SKSpriteNode!
    private var walkFrames: [SKTexture] = []
    private var sitSideOpen: SKTexture!
    private var sitSideBlink: SKTexture!
    private var sitFrontOpen: SKTexture?
    private var sitFrontBlink: SKTexture?
    private var isFacingRight = true

    private let walkAnimationKey = "mabel.walk.animation"
    private let walkMoveKey = "mabel.walk.move"
    private let decisionKey = "mabel.decision"
    private let breathingKey = "mabel.breathing"
    private let tailSwayKey = "mabel.tailSway"
    private let earTwitchKey = "mabel.earTwitch"
    private let cursorLookKey = "mabel.cursorLook"

    public init(size: CGSize, configuration: MabelConfiguration = MabelConfiguration()) {
        self.config = configuration
        super.init(size: size)
        scaleMode = .resizeFill
    }

    @available(*, unavailable)
    public required init?(coder aDecoder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    public override func didMove(to view: SKView) {
        backgroundColor = .clear
        loadTextures()
        setupCatIfNeeded()
        enterIdle()
    }

    public override func didChangeSize(_ oldSize: CGSize) {
        super.didChangeSize(oldSize)
        keepInsideBounds()
    }

    private func loadTextures() {
        let sheet = SKTexture(imageNamed: config.spriteName)
        sheet.filteringMode = .linear
        let frameWidth = 1.0 / CGFloat(config.totalFrames)
        let allFrames = (0..<config.totalFrames).map { index -> SKTexture in
            let texture = SKTexture(
                rect: CGRect(x: CGFloat(index) * frameWidth, y: 0, width: frameWidth, height: 1),
                in: sheet
            )
            texture.filteringMode = .linear
            return texture
        }

        walkFrames = Array(allFrames[config.walkFrameRange])
        sitSideOpen = allFrames[config.sitSideOpenFrame]
        sitSideBlink = allFrames[config.sitSideBlinkFrame]

        if let frontIndex = config.sitFrontOpenFrame, allFrames.indices.contains(frontIndex) {
            sitFrontOpen = allFrames[frontIndex]
        }
        if let frontBlinkIndex = config.sitFrontBlinkFrame, allFrames.indices.contains(frontBlinkIndex) {
            sitFrontBlink = allFrames[frontBlinkIndex]
        }
    }

    private func setupCatIfNeeded() {
        guard cat == nil else { return }
        cat = SKSpriteNode(texture: sitSideOpen)
        cat.size = config.displaySize
        cat.anchorPoint = CGPoint(x: 0.5, y: 0.0)
        cat.position = CGPoint(x: size.width / 2.0, y: config.floorY)
        cat.zPosition = 100
        addChild(cat)
    }

    // MARK: - Public Controls

    public func start() { enterIdle() }
    public func pauseMabel() { cat?.removeAllActions() }
    public func resumeMabel() { enterIdle() }

    // MARK: - State Machine

    private func enterWalking() {
        state = .walking
        removeBehaviorActions()

        let walkRight = shouldWalkRight()
        faceRight(walkRight)
        cat.texture = walkFrames.first

        let distance = nextWalkDistance(facingRight: walkRight)
        let duration = TimeInterval(abs(distance) / max(config.walkSpeedPointsPerSecond, 1))

        let animation = SKAction.repeatForever(
            SKAction.animate(with: walkFrames, timePerFrame: 0.075, resize: false, restore: false)
        )

        let move = SKAction.moveBy(x: distance, y: 0, duration: duration)
        move.timingMode = .linear

        cat.run(animation, withKey: walkAnimationKey)
        cat.run(.sequence([move, .run { [weak self] in self?.enterIdle() }]), withKey: walkMoveKey)
    }

    private func enterIdle() {
        state = .sitting
        removeBehaviorActions()
        cat.texture = sitSideOpen

        startBreathing()
        startTailSwayFallback()
        scheduleEarTwitch()

        let wait = SKAction.wait(forDuration: TimeInterval.random(in: config.idleDecisionRange))
        let decide = SKAction.run { [weak self] in
            guard let self else { return }
            let roll = Int.random(in: 1...100)
            if roll <= 50 {
                self.enterWalking()
            } else if roll <= 78 {
                self.enterBlink()
            } else {
                self.enterLookAtCursor()
            }
        }
        cat.run(.sequence([wait, decide]), withKey: decisionKey)
    }

    private func enterBlink() {
        state = .blinking
        removeBehaviorActions(keepBreathing: true)

        let open = sitSideOpen!
        let closed = sitSideBlink!

        cat.run(.sequence([
            .setTexture(open),
            .wait(forDuration: TimeInterval.random(in: 0.8...2.0)),
            .setTexture(closed),
            .wait(forDuration: 0.12),
            .setTexture(open),
            .wait(forDuration: TimeInterval.random(in: 0.4...1.2)),
            .run { [weak self] in self?.enterIdle() }
        ]), withKey: decisionKey)
    }

    private func enterLookAtCursor() {
        state = .lookingAtCursor
        removeBehaviorActions(keepBreathing: true)

        let open = sitFrontOpen ?? sitSideOpen!
        let closed = sitFrontBlink ?? sitSideBlink!
        cat.texture = open
        cat.xScale = abs(cat.xScale)

        var actions: [SKAction] = [
            .wait(forDuration: TimeInterval.random(in: 0.6...1.4))
        ]

        if Bool.random() {
            actions.append(contentsOf: [
                .setTexture(closed),
                .wait(forDuration: 0.12),
                .setTexture(open)
            ])
        }

        actions.append(contentsOf: [
            .wait(forDuration: TimeInterval.random(in: 0.5...1.4)),
            .run { [weak self] in self?.enterIdle() }
        ])

        cat.run(.sequence(actions), withKey: cursorLookKey)
    }

    // MARK: - Micro Animations

    private func startBreathing() {
        cat.removeAction(forKey: breathingKey)
        let inhale = SKAction.scaleY(to: 1.015, duration: 1.2)
        let exhale = SKAction.scaleY(to: 1.0, duration: 1.2)
        inhale.timingMode = .easeInEaseOut
        exhale.timingMode = .easeInEaseOut
        cat.run(.repeatForever(.sequence([inhale, exhale])), withKey: breathingKey)
    }

    private func startTailSwayFallback() {
        // Placeholder overlay until tail is exported as a separate asset.
        // This tiny rotation sells a soft idle tail/fur movement without distorting the whole cat too much.
        cat.removeAction(forKey: tailSwayKey)
        let left = SKAction.rotate(toAngle: -0.012, duration: 0.9)
        let right = SKAction.rotate(toAngle: 0.012, duration: 0.9)
        left.timingMode = .easeInEaseOut
        right.timingMode = .easeInEaseOut
        cat.run(.repeatForever(.sequence([left, right])), withKey: tailSwayKey)
    }

    private func scheduleEarTwitch() {
        cat.removeAction(forKey: earTwitchKey)
        let wait = SKAction.wait(forDuration: TimeInterval.random(in: 6...18))
        let twitch = SKAction.sequence([
            .rotate(byAngle: 0.012, duration: 0.05),
            .rotate(byAngle: -0.024, duration: 0.08),
            .rotate(byAngle: 0.012, duration: 0.05)
        ])
        cat.run(.sequence([wait, twitch, .run { [weak self] in self?.scheduleEarTwitch() }]), withKey: earTwitchKey)
    }

    // MARK: - Mouse Interaction

    public override func mouseMoved(with event: NSEvent) {
        guard cat != nil else { return }
        let location = event.location(in: self)
        let distance = hypot(location.x - cat.position.x, location.y - cat.position.y)
        if distance < config.cursorInterestRadius, state != .lookingAtCursor {
            enterLookAtCursor()
        }
    }

    public override func mouseDown(with event: NSEvent) {
        enterLookAtCursor()
    }

    // MARK: - Helpers

    private func faceRight(_ right: Bool) {
        isFacingRight = right
        let sign: CGFloat = right ? 1 : -1
        cat.xScale = abs(cat.xScale) * sign
    }

    private func shouldWalkRight() -> Bool {
        keepInsideBounds()
        let margin = config.displaySize.width / 2
        if cat.position.x < margin + 40 { return true }
        if cat.position.x > size.width - margin - 40 { return false }
        return Bool.random()
    }

    private func nextWalkDistance(facingRight: Bool) -> CGFloat {
        let requested = CGFloat.random(in: 220...520) * (facingRight ? 1 : -1)
        let minX = config.displaySize.width / 2
        let maxX = max(minX, size.width - config.displaySize.width / 2)
        let target = min(max(cat.position.x + requested, minX), maxX)
        return target - cat.position.x
    }

    private func keepInsideBounds() {
        guard cat != nil else { return }
        let minX = config.displaySize.width / 2
        let maxX = max(minX, size.width - config.displaySize.width / 2)
        cat.position.x = min(max(cat.position.x, minX), maxX)
        cat.position.y = config.floorY
    }

    private func removeBehaviorActions(keepBreathing: Bool = false) {
        [walkAnimationKey, walkMoveKey, decisionKey, tailSwayKey, earTwitchKey, cursorLookKey].forEach {
            cat.removeAction(forKey: $0)
        }
        if !keepBreathing { cat.removeAction(forKey: breathingKey) }
        cat.zRotation = 0
    }
}
