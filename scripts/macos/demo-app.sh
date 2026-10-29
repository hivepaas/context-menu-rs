#!/bin/sh
# Builds the demo as an app holding the native extension and its helper,
# signs it, and installs its menu, which tells the system of the extension
# and moves the menu to Finder's own: see docs/macos.md.
#
#   IDENTITY='Developer ID Application: Name (TEAMID)' scripts/macos/demo-app.sh
#   scripts/macos/demo-app.sh --remove
#
# The identity is a code-signing one in your keychain (`security
# find-identity -v -p codesigning`): the extension's app group starts with
# its team's id, so it cannot be ad hoc.
set -eu
cd "$(dirname "$0")/../.."
OUT=target/context-menu-demo
APP=$OUT/ContextMenuDemo.app
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

if [ "${1:-}" = --remove ]; then
  if [ -x "$APP/Contents/MacOS/demo" ]; then
    "$APP/Contents/MacOS/demo" uninstall
  fi
  # The extension, which Finder may still run, then the system's records of it.
  pkill -f "$APP/Contents/PlugIns/ContextMenu.appex/" 2>/dev/null || true
  pluginkit -r "$APP/Contents/PlugIns/ContextMenu.appex" 2>/dev/null || true
  "$LSREGISTER" -u "$APP" 2>/dev/null || true
  rm -rf "$OUT"
  echo "removed $APP"
  exit 0
fi

: "${IDENTITY:?set IDENTITY to a code-signing identity, such as 'Developer ID Application: Name (TEAMID)'}"
TEAM=$(printf %s "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p')
[ -n "$TEAM" ] || { echo "no team id at the end of IDENTITY: $IDENTITY" >&2; exit 2; }

cargo build --example demo
cargo build -p context-menu-finder

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp target/debug/examples/demo "$APP/Contents/MacOS/demo"
for bundle in Contents/PlugIns/ContextMenu.appex Contents/Helpers/ContextMenuHelper.app; do
  mkdir -p "$APP/$bundle/Contents/MacOS"
  cp target/debug/context-menu-finder "$APP/$bundle/Contents/MacOS/"
done
target/debug/examples/demo bundle "$APP" "$TEAM"

# Inside out: what a bundle holds is signed before the bundle.
sign() { codesign --force --sign "$IDENTITY" --options runtime --timestamp=none "$@"; }
sign --entitlements "$OUT/helper.entitlements" "$APP/Contents/Helpers/ContextMenuHelper.app"
sign --entitlements "$OUT/extension.entitlements" "$APP/Contents/PlugIns/ContextMenu.appex"
sign "$APP"
codesign --verify --deep --strict "$APP"

# Run from the app, install finds the extension in it.
"$APP/Contents/MacOS/demo" install
