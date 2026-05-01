#if false
// Example app entry point. Copy this into your macOS app target if you want a tiny menu-bar wrapper.

import AppKit
import MabelCompanion

@main
final class MabelAppDelegate: NSObject, NSApplicationDelegate {
    private var menuBar: MabelMenuBarController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        menuBar = MabelMenuBarController()
        menuBar?.start()
    }
}
#endif
