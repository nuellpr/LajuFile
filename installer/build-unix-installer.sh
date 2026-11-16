#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)"
TAG="${TAG_OVERRIDE:-v$VERSION}"
OS_NAME="$(uname -s)"
ARCH_NAME="$(uname -m)"

mkdir -p dist
cargo build --release -p lajufile-app --bin lajufile --bin lajufile-setup

if [[ "$OS_NAME" == "Darwin" ]]; then
  ARCH_TAG="x64"
  if [[ "$ARCH_NAME" == "arm64" ]]; then
    ARCH_TAG="arm64"
  fi
  RAW_SETUP="dist/LajuFile-${TAG}-macos-${ARCH_TAG}-setup"
  DMG_OUT="dist/LajuFile-${TAG}-macos-${ARCH_TAG}-setup.dmg"
  STAGE="$(mktemp -d)"
  APP_BUNDLE="$STAGE/LajuFile Setup.app"

  ./target/release/lajufile-setup \
    --pack \
    --stub target/release/lajufile-setup \
    --app target/release/lajufile \
    --ext extension \
    --license LICENSE \
    --readme README.md \
    --out "$RAW_SETUP"
  chmod +x "$RAW_SETUP"

  mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"
  cp "$RAW_SETUP" "$APP_BUNDLE/Contents/MacOS/lajufile-setup"
  chmod +x "$APP_BUNDLE/Contents/MacOS/lajufile-setup"
  cp assets/icons/icon-256.png "$APP_BUNDLE/Contents/Resources/icon.png"
  cat > "$APP_BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>lajufile-setup</string>
  <key>CFBundleIdentifier</key><string>io.lajufile.setup</string>
  <key>CFBundleName</key><string>LajuFile Setup</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
  codesign --force --deep --sign - "$APP_BUNDLE" || true
  hdiutil create -volname "LajuFile ${TAG} Setup" -srcfolder "$STAGE" -ov -format UDZO "$DMG_OUT"
  rm -rf "$STAGE"
  echo "Built macOS Installer: $DMG_OUT and $RAW_SETUP"
else
  ARCH_TAG="x64"
  if [[ "$ARCH_NAME" == "aarch64" ]]; then
    ARCH_TAG="arm64"
  fi
  OUT_RUN="dist/LajuFile-${TAG}-linux-${ARCH_TAG}-setup.run"
  ./target/release/lajufile-setup \
    --pack \
    --stub target/release/lajufile-setup \
    --app target/release/lajufile \
    --ext extension \
    --license LICENSE \
    --readme README.md \
    --out "$OUT_RUN"
  chmod +x "$OUT_RUN"
  echo "Built Linux Installer: $OUT_RUN"
fi
