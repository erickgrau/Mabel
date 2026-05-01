#!/usr/bin/env python3
"""
Set the Finder icon of a .dmg file to the Mabel app icon.

Why this exists: macOS shows a generic disk-image icon for any .dmg by default.
Tauri's bundler sets the icon for the *mounted volume* via --volicon, but not
for the .dmg file itself. This script does the second half using PyObjC's
NSWorkspace.setIcon_forFile_options_, which is the official Cocoa API for this.

Run order matters. Apple's notarization stapler writes a ticket into the .dmg;
mutating the file with this script after stapling invalidates the staple. So:

    1. npm run tauri build
    2. python3 scripts/set-dmg-icon.py <path-to.dmg>
    3. xcrun notarytool submit --keychain-profile AC_PASSWORD --wait <dmg>
    4. xcrun stapler staple <dmg>

Usage:
    python3 scripts/set-dmg-icon.py path/to/Mabel_X.Y.Z_aarch64.dmg
"""

import sys
from pathlib import Path

try:
    from AppKit import NSImage, NSWorkspace
except ImportError:
    sys.stderr.write(
        "PyObjC not available. Install with: pip3 install --user pyobjc-framework-Cocoa\n"
        "Or use the system Python (PyObjC ships with macOS Python).\n"
    )
    sys.exit(1)


def set_icon(dmg_path: str, icon_path: str) -> None:
    img = NSImage.alloc().initWithContentsOfFile_(icon_path)
    if img is None:
        raise SystemExit(f"Could not load icon: {icon_path}")
    ok = NSWorkspace.sharedWorkspace().setIcon_forFile_options_(img, dmg_path, 0)
    if not ok:
        raise SystemExit(f"setIcon failed for: {dmg_path}")
    print(f"Set icon on {dmg_path}")


def main() -> None:
    if len(sys.argv) < 2:
        sys.stderr.write(__doc__)
        sys.exit(2)
    dmg = Path(sys.argv[1]).resolve()
    if not dmg.exists():
        raise SystemExit(f"DMG not found: {dmg}")
    icon = Path(__file__).parent.parent / "src-tauri" / "icons" / "icon.icns"
    if not icon.exists():
        raise SystemExit(f"Icon not found at expected path: {icon}")
    set_icon(str(dmg), str(icon))


if __name__ == "__main__":
    main()
