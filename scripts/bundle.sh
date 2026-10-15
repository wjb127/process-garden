#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
CARGO_BUILD_JOBS=2 cargo build --locked --release
app='dist/Process Garden.app'
mkdir -p "$app/Contents/MacOS"
cp target/release/process-garden "$app/Contents/MacOS/process-garden"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Process Garden</string>
<key>CFBundleDisplayName</key><string>Process Garden</string>
<key>CFBundleIdentifier</key><string>com.conabuild.process-garden</string>
<key>CFBundleVersion</key><string>3</string>
<key>CFBundleShortVersionString</key><string>0.3.0</string>
<key>CFBundleExecutable</key><string>process-garden</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$app"
printf '%s\n' "$PWD/$app"
