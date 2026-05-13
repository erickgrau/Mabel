#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

VERSION=$(node -p 'require("./package.json").version')
TAG="v$VERSION"
IDENTITY="${MABEL_SIGNING_IDENTITY:-Developer ID Application: Erick Grau (DF9FB764AR)}"
NOTARY_PROFILE="${MABEL_NOTARY_PROFILE:-AC_PASSWORD}"
LOCAL_CONFIG="${MABEL_TAURI_CONFIG:-src-tauri/tauri.local.conf.json}"
UPDATER_KEY="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/mabel-updater.key}"
DMG="src-tauri/target/release/bundle/dmg/Mabel_${VERSION}_aarch64.dmg"
APP="src-tauri/target/release/bundle/macos/Mabel.app"
UPDATER="src-tauri/target/release/bundle/macos/Mabel.app.tar.gz"
SIG="$UPDATER.sig"
LATEST="src-tauri/target/release/bundle/macos/latest.json"

die() {
  echo "release-macos: $*" >&2
  exit 1
}

[[ "$(git rev-parse --abbrev-ref HEAD)" == "main" ]] || die "must run from main"
git diff --quiet || die "working tree has unstaged changes"
git diff --cached --quiet || die "working tree has staged changes"
[[ -f "$LOCAL_CONFIG" ]] || die "missing local signing config: $LOCAL_CONFIG"
[[ -f "$UPDATER_KEY" ]] || die "missing updater key: $UPDATER_KEY"
grep -q "^## v$VERSION " docs/whatsnew.md || die "docs/whatsnew.md missing ## v$VERSION entry"

TAURI_SIGNING_PRIVATE_KEY="$(cat "$UPDATER_KEY")" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" \
npm run tauri build -- --config "$LOCAL_CONFIG" --bundles app

rm -f /tmp/Mabel.app.zip
(
  cd src-tauri/target/release/bundle/macos
  ditto -c -k --keepParent Mabel.app /tmp/Mabel.app.zip
)
xcrun notarytool submit /tmp/Mabel.app.zip --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$APP"
spctl -a -vv -t exec "$APP"

WORK=$(mktemp -d /tmp/mabel-dmg.XXXXXX)
trap 'rm -rf "$WORK"' EXIT
SRC="$WORK/src"
mkdir -p "$SRC"
ditto "$APP" "$SRC/Mabel.app"
ln -s /Applications "$SRC/Applications"
cp src-tauri/icons/icon.icns "$SRC/.VolumeIcon.icns"
SetFile -a C "$SRC"
mkdir -p "$(dirname "$DMG")"
rm -f "$DMG"
hdiutil create -volname "Mabel" -srcfolder "$SRC" -ov -format UDZO "$DMG"
scripts/set-dmg-icon.sh "$DMG"
codesign --sign "$IDENTITY" --timestamp "$DMG"
xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
xcrun stapler staple "$DMG"
spctl -a -t open --context context:primary-signature -vv "$DMG"
xcrun stapler validate "$DMG"

node <<'NODE'
const fs = require('fs');
const version = require('./package.json').version;
const sig = fs.readFileSync('src-tauri/target/release/bundle/macos/Mabel.app.tar.gz.sig', 'utf8').trim();
const manifest = {
  version,
  notes: fs.readFileSync('docs/whatsnew.md', 'utf8')
    .split(`## v${version}`)[1]
    ?.split('\n## v')[0]
    ?.trim() || `Mabel v${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'darwin-aarch64': {
      signature: sig,
      url: `https://github.com/erickgrau/Mabel/releases/download/v${version}/Mabel.app.tar.gz`,
    },
  },
};
fs.writeFileSync('src-tauri/target/release/bundle/macos/latest.json', `${JSON.stringify(manifest, null, 2)}\n`);
NODE

gh release create "$TAG" "$DMG" "$UPDATER" "$SIG" "$LATEST" \
  --title "Mabel $TAG" \
  --notes-file <(awk "/^## v$VERSION /{flag=1; next} /^## v/{flag=0} flag" docs/whatsnew.md) \
  --target main \
  --latest

echo "Published $TAG"
