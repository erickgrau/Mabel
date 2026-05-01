import CoreGraphics
import Foundation

public struct MabelConfiguration: Sendable {
    public var spriteName: String
    public var totalFrames: Int
    public var walkFrameRange: ClosedRange<Int>
    public var sitSideOpenFrame: Int
    public var sitSideBlinkFrame: Int
    public var sitFrontOpenFrame: Int?
    public var sitFrontBlinkFrame: Int?
    public var displaySize: CGSize
    public var floorY: CGFloat
    public var walkSpeedPointsPerSecond: CGFloat
    public var idleDecisionRange: ClosedRange<Double>
    public var cursorInterestRadius: CGFloat

    public init(
        spriteName: String = "mabel_sprite_4096x512",
        totalFrames: Int = 8,
        walkFrameRange: ClosedRange<Int> = 0...5,
        sitSideOpenFrame: Int = 6,
        sitSideBlinkFrame: Int = 7,
        sitFrontOpenFrame: Int? = nil,
        sitFrontBlinkFrame: Int? = nil,
        displaySize: CGSize = CGSize(width: 160, height: 160),
        floorY: CGFloat = 92,
        walkSpeedPointsPerSecond: CGFloat = 58,
        idleDecisionRange: ClosedRange<Double> = 1.4...4.8,
        cursorInterestRadius: CGFloat = 180
    ) {
        self.spriteName = spriteName
        self.totalFrames = totalFrames
        self.walkFrameRange = walkFrameRange
        self.sitSideOpenFrame = sitSideOpenFrame
        self.sitSideBlinkFrame = sitSideBlinkFrame
        self.sitFrontOpenFrame = sitFrontOpenFrame
        self.sitFrontBlinkFrame = sitFrontBlinkFrame
        self.displaySize = displaySize
        self.floorY = floorY
        self.walkSpeedPointsPerSecond = walkSpeedPointsPerSecond
        self.idleDecisionRange = idleDecisionRange
        self.cursorInterestRadius = cursorInterestRadius
    }
}
