import AppKit

public final class MabelMenuBarController: NSObject {
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    private let desktopController: MabelDesktopWindowController

    public init(desktopController: MabelDesktopWindowController = MabelDesktopWindowController()) {
        self.desktopController = desktopController
        super.init()
        configureMenu()
    }

    public func start() {
        desktopController.show()
    }

    private func configureMenu() {
        statusItem.button?.title = "Mabel"

        let menu = NSMenu()
        menu.addItem(NSMenuItem(title: "Show Mabel", action: #selector(showMabel), keyEquivalent: ""))
        menu.addItem(NSMenuItem(title: "Hide Mabel", action: #selector(hideMabel), keyEquivalent: ""))
        menu.addItem(.separator())
        menu.addItem(NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "q"))

        menu.items.forEach { $0.target = self }
        statusItem.menu = menu
    }

    @objc private func showMabel() {
        desktopController.show()
    }

    @objc private func hideMabel() {
        desktopController.hideMabel()
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}
