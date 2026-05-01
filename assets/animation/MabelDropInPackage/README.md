# Mabel Companion Drop-in Package

This package contains a drop-in SpriteKit companion system for **Mabel**, including:

- `MabelScene.swift` state machine
- `MabelDesktopWindowController.swift` transparent floating macOS window
- `MabelMenuBarController.swift` basic menu-bar controls
- `mabel_sprite_4096x512.png` sprite sheet asset
- breathing, idle sway, rare ear twitch, random direction changes, pause/sit behavior, and cursor-aware look behavior

## Current Sprite Sheet

The included sprite sheet is the current 8-frame production sheet:

```text
mabel_sprite_4096x512.png
4096 x 512
8 frames
512 x 512 each
```

Frame map:

```text
0-5 = walk cycle
6   = side sit, eyes open
7   = side sit, blink
```

The state machine is also ready for a future 16-frame sheet. When you have a clean 16-frame asset, use this configuration:

```swift
let config = MabelConfiguration(
    spriteName: "mabel_sprite_8192x512",
    totalFrames: 16,
    walkFrameRange: 0...11,
    sitSideOpenFrame: 12,
    sitSideBlinkFrame: 13,
    sitFrontOpenFrame: 14,
    sitFrontBlinkFrame: 15
)
```

## Install Option A: Drag into an existing Xcode app target

1. Drag `Sources/MabelCompanion/*.swift` into your macOS target.
2. Drag `Resources/Assets.xcassets` into your app target or copy the image set into your existing asset catalog.
3. Create and show the desktop controller:

```swift
private var mabelController: MabelDesktopWindowController?

func applicationDidFinishLaunching(_ notification: Notification) {
    NSApp.setActivationPolicy(.accessory)
    mabelController = MabelDesktopWindowController()
    mabelController?.show()
}
```

## Install Option B: Swift Package

Add this folder as a local Swift Package in Xcode:

```text
File > Add Package Dependencies > Add Local...
```

Then import:

```swift
import MabelCompanion
```

## Notes

- The tail sway is currently a subtle full-sprite fallback because the tail is baked into the sitting frame.
- For a true independent tail animation, export body and tail as separate transparent PNG layers.
- `window.ignoresMouseEvents` is intentionally set to `false` so Mabel can react to cursor movement/clicks. Change it to `true` if you want the cat to never intercept clicks.
- `window.level = .screenSaver` keeps Mabel visible above most windows. Use `.floating` if that feels too aggressive.

## Files

```text
Package.swift
Sources/MabelCompanion/MabelConfiguration.swift
Sources/MabelCompanion/MabelState.swift
Sources/MabelCompanion/MabelScene.swift
Sources/MabelCompanion/MabelDesktopWindowController.swift
Sources/MabelCompanion/MabelMenuBarController.swift
Sources/MabelCompanion/MabelAppExample.swift
Resources/Assets.xcassets/mabel_sprite_4096x512.imageset/mabel_sprite_4096x512.png
Resources/Sprites/mabel_sprite_4096x512.png
```
