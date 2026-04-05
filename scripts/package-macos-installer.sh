#!/usr/bin/env bash
set -euo pipefail

CONFIGURATION="${1:-release}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_ROOT="${REPO_ROOT}/dist/macos"
APP_STAGING_ROOT="${OUTPUT_ROOT}/FeishuBridgeSetup.app"
CONTENTS_DIR="${APP_STAGING_ROOT}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
SKIP_EXTENSION_PACKAGE="${SKIP_EXTENSION_PACKAGE:-0}"

echo "[macos-installer] repo root: ${REPO_ROOT}"
echo "[macos-installer] output dir: ${OUTPUT_ROOT}"

mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

if [[ "${SKIP_EXTENSION_PACKAGE}" != "1" ]]; then
  echo "[macos-installer] packaging companion extension"
  pushd "${REPO_ROOT}/vscode-agent-bridge" >/dev/null
  npm install
  npm run compile
  npm run package:vsix
  popd >/dev/null
fi

echo "[macos-installer] building Rust binaries"
if [[ "${CONFIGURATION}" == "release" ]]; then
  cargo build --bin bridge-cli --bin setup-gui --release
  TARGET_DIR="release"
else
  cargo build --bin bridge-cli --bin setup-gui
  TARGET_DIR="debug"
fi

cp "${REPO_ROOT}/target/${TARGET_DIR}/bridge-cli" "${RESOURCES_DIR}/bridge-cli"
cp "${REPO_ROOT}/target/${TARGET_DIR}/setup-gui" "${MACOS_DIR}/setup-gui"

cat > "${CONTENTS_DIR}/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>setup-gui</string>
  <key>CFBundleIdentifier</key>
  <string>com.beanharbor.feishubridgesetup</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Feishu Bridge Setup</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
PLIST

VSIX_SOURCE=""
for candidate in \
  "${REPO_ROOT}/vscode-agent-bridge/dist/feishu-agent-bridge.vsix" \
  "${REPO_ROOT}/vscode-agent-bridge/feishu-agent-bridge.vsix"
do
  if [[ -f "$candidate" ]]; then
    VSIX_SOURCE="$candidate"
    break
  fi
done

if [[ -n "${VSIX_SOURCE}" ]]; then
  cp "${VSIX_SOURCE}" "${RESOURCES_DIR}/feishu-agent-bridge.vsix"
else
  echo "[macos-installer] warning: companion extension VSIX not found; setup-gui will fall back to Marketplace install"
fi

DMG_PATH="${OUTPUT_ROOT}/FeishuBridgeSetup.dmg"
mkdir -p "${OUTPUT_ROOT}"

echo "[macos-installer] creating dmg"
hdiutil create -volname "Feishu Bridge Setup" -srcfolder "${APP_STAGING_ROOT}" -ov -format UDZO "${DMG_PATH}"
echo "[macos-installer] done: ${DMG_PATH}"