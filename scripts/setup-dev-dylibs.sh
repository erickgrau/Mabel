#!/bin/sh
# Run once after cloning the repo, or whenever target/ is wiped.
# Ensures the whisper-cpp sidecar can find its dylibs in dev builds.
#
# In prod, Tauri bundles src-tauri/dylibs/ into Mabel.app/Contents/Frameworks/
# automatically (see bundle.macOS.frameworks in tauri.conf.json).
# In dev, the sidecar at target/debug/whisper-cpp expects dylibs at
# target/Frameworks/, so we symlink that to src-tauri/dylibs.

set -e
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
mkdir -p "$ROOT/src-tauri/target"
rm -rf "$ROOT/src-tauri/target/Frameworks"
ln -s ../dylibs "$ROOT/src-tauri/target/Frameworks"
echo "linked $ROOT/src-tauri/target/Frameworks -> ../dylibs"
