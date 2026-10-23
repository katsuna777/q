#!/bin/sh
# Build Otpuska.app into dist/. Usage: packaging/macos/bundle.sh
#   UNIVERSAL=1 packaging/macos/bundle.sh   # arm64 + x86_64 in one binary (needs both rust targets)
#   DIST_DIR=dist/next packaging/macos/bundle.sh   # build elsewhere (default: dist), e.g. while dist/Otpuska.app is running
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
[ -f ./dev-env.sh ] && . ./dev-env.sh

if [ "${UNIVERSAL:-0}" = "1" ]; then
  cargo build --release -p otpuska-gui --target aarch64-apple-darwin
  cargo build --release -p otpuska-gui --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output target/universal/otpuska \
    target/aarch64-apple-darwin/release/otpuska target/x86_64-apple-darwin/release/otpuska
  bin="target/universal/otpuska"
else
  cargo build --release -p otpuska-gui
  bin="target/release/otpuska"
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
dist="${DIST_DIR:-dist}"
app="$dist/Otpuska.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/otpuska"
cp crates/otpuska-gui/assets/icon.icns "$app/Contents/Resources/icon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Отпуска</string>
  <key>CFBundleDisplayName</key><string>Отпуска</string>
  <key>CFBundleIdentifier</key><string>dev.otpuska.desktop</string>
  <key>CFBundleExecutable</key><string>otpuska</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleDevelopmentRegion</key><string>ru</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.business</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true
echo "built $app ($(du -sh "$app" | cut -f1))"
