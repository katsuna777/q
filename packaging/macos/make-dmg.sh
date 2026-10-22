#!/bin/sh
# Wrap dist/Otpuska.app into a disk image: dist/Otpuska-<version>-macos.dmg
# (run packaging/macos/bundle.sh first).
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
stage="dist/dmg"
rm -rf "$stage"
mkdir -p "$stage"
cp -R dist/Otpuska.app "$stage/"
ln -s /Applications "$stage/Applications"
out="dist/Otpuska-$version-macos.dmg"
rm -f "$out"
hdiutil create -volname "Отпуска $version" -srcfolder "$stage" -ov -format UDZO "$out" >/dev/null
rm -rf "$stage"
echo "built $out ($(du -sh "$out" | cut -f1))"
