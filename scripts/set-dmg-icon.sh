#!/bin/sh
# Set the Finder icon of a .dmg file to the Mabel app icon.
#
# Why this exists: macOS shows a generic disk-image icon for any .dmg by
# default. Tauri's bundler sets the icon for the *mounted volume* via
# --volicon, but not for the .dmg file itself. This script applies the icon
# to the .dmg file using JavaScript for Automation (JXA), which has full
# Cocoa access through osascript — no Python or PyObjC needed.
#
# Run order matters. Apple's notarization stapler writes a ticket into the
# .dmg; mutating the file with this script after stapling invalidates the
# staple. Always run BEFORE notarytool submit:
#
#   1. npm run tauri build
#   2. scripts/set-dmg-icon.sh <path-to.dmg>
#   3. xcrun notarytool submit --keychain-profile AC_PASSWORD --wait <dmg>
#   4. xcrun stapler staple <dmg>
#
# Usage:
#   scripts/set-dmg-icon.sh path/to/Mabel_X.Y.Z_aarch64.dmg

set -e

if [ -z "$1" ]; then
    echo "usage: $0 path/to/file.dmg" >&2
    exit 2
fi

DMG=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
ICON="$SCRIPT_DIR/../src-tauri/icons/icon.icns"

if [ ! -f "$DMG" ]; then
    echo "DMG not found: $DMG" >&2
    exit 1
fi
if [ ! -f "$ICON" ]; then
    echo "Icon not found: $ICON" >&2
    exit 1
fi

osascript -l JavaScript - "$DMG" "$ICON" <<'JXA'
function run(args) {
    ObjC.import('Cocoa');
    const dmg = args[0];
    const iconPath = args[1];
    const img = $.NSImage.alloc.initWithContentsOfFile(iconPath);
    if (img.isNil()) {
        throw new Error('Could not load icon: ' + iconPath);
    }
    const ok = $.NSWorkspace.sharedWorkspace.setIconForFileOptions(img, dmg, 0);
    if (!ok) {
        throw new Error('setIcon failed for: ' + dmg);
    }
    return 'set icon on ' + dmg;
}
JXA
