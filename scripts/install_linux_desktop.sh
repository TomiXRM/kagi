#!/usr/bin/env bash
# Offline desktop integration for the Kagi AppImage (ADR-0047).
#
# Installs the AppImage into ~/.local/bin, registers a hicolor icon and a
# .desktop entry, and best-effort refreshes the desktop/icon caches. No network
# access (no curl/wget) — everything ships inside the distributed zip.
#
# Usage:
#   bash install_linux_desktop.sh [/path/to/Kagi-<arch>.AppImage]
# With no argument it auto-detects a Kagi-*.AppImage next to this script.
set -euo pipefail

APP_NAME="Kagi"
APP_ID="com.tomixrm.kagi"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

APPIMAGE_PATH="${1:-}"
if [[ -z "${APPIMAGE_PATH}" ]]; then
  # Search, in order: the script's own dir, the unzip root (where the
  # distributed `kagi_Linux-AppImage_<arch>.zip` places the AppImage next to
  # `scripts/`), the current working dir, and `<root>/dist` + `target/dist`
  # (dev builds).
  APPIMAGE_PATH="$(find \
      "${SCRIPT_DIR}" \
      "${ROOT_DIR}" \
      "${PWD}" \
      "${ROOT_DIR}/dist" \
      "${ROOT_DIR}/target/dist" \
      -maxdepth 1 -name "${APP_NAME}-*.AppImage" -print -quit 2>/dev/null || true)"
fi

if [[ -z "${APPIMAGE_PATH}" || ! -f "${APPIMAGE_PATH}" ]]; then
  echo "Usage: $0 /path/to/${APP_NAME}-<arch>.AppImage" >&2
  exit 1
fi

ICON_SOURCE=""
# kagi.png ships at the zip root (next to scripts/); also accept it beside the
# script, in the cwd, or the in-repo source icon (dev).
for cand in \
    "${SCRIPT_DIR}/kagi.png" \
    "${ROOT_DIR}/kagi.png" \
    "${PWD}/kagi.png" \
    "${ROOT_DIR}/assets/icon/icon_512x512.png"; do
  if [[ -f "${cand}" ]]; then
    ICON_SOURCE="${cand}"
    break
  fi
done
if [[ -z "${ICON_SOURCE}" ]]; then
  echo "Could not find ${APP_NAME} icon (kagi.png)." >&2
  exit 1
fi

INSTALL_DIR="${HOME}/.local/bin"
APPLICATIONS_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/512x512/apps"

mkdir -p "${INSTALL_DIR}" "${APPLICATIONS_DIR}" "${ICON_DIR}"

INSTALLED_APPIMAGE="${INSTALL_DIR}/${APP_NAME}.AppImage"
cp "${APPIMAGE_PATH}" "${INSTALLED_APPIMAGE}"
chmod +x "${INSTALLED_APPIMAGE}"

cp "${ICON_SOURCE}" "${ICON_DIR}/kagi.png"

cat > "${APPLICATIONS_DIR}/${APP_ID}.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=${APP_NAME}
Comment=Safety-first Git GUI client
Exec=${INSTALLED_APPIMAGE} %F
Icon=kagi
Terminal=false
Categories=Development;
StartupWMClass=${APP_NAME}
EOF

chmod 644 "${APPLICATIONS_DIR}/${APP_ID}.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${APPLICATIONS_DIR}" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Installed ${APP_NAME}"
echo "AppImage: ${INSTALLED_APPIMAGE}"
echo "Desktop entry: ${APPLICATIONS_DIR}/${APP_ID}.desktop"
echo "If ${INSTALL_DIR} is not on your PATH, add it to launch '${APP_NAME}' from a terminal."
