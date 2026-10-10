#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# Install / refresh "Omni Web" (the BROWSER-BODY-0 Web app) as a desktop app for this user:
#   ~/.local/lib/omni-web/omni-ui (a copy: a rebuild in target/ never breaks the installed app), ~/.local/bin/omni-web,
#   ~/.local/share/applications/omni-web.desktop (menu entry; offered for http/https links — NOT made the default browser).
set -e
cd "$(dirname "$0")/.."
D="$HOME/.local/lib/omni-web"
mkdir -p "$D" "$HOME/.local/bin" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/scalable/apps"
cp target/release/omni-ui "$D/omni-ui"
printf '#!/bin/sh\nexec "$HOME/.local/lib/omni-web/omni-ui" run web "$@"\n' > "$HOME/.local/bin/omni-web"; chmod +x "$HOME/.local/bin/omni-web"
cat > "$HOME/.local/share/icons/hicolor/scalable/apps/omni-web.svg" <<'S'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><circle cx="32" cy="32" r="29" fill="#0b4fa8"/><circle cx="32" cy="32" r="21" fill="none" stroke="#fff" stroke-width="4"/><path d="M11 32h42M32 11c-8 7-8 35 0 42M32 11c8 7 8 35 0 42" fill="none" stroke="#fff" stroke-width="3"/></svg>
S
cat > "$HOME/.local/share/applications/omni-web.desktop" <<S
[Desktop Entry]
Type=Application
Name=Omni Web
GenericName=Web Browser
Comment=Light-use browser on the OMNI-RENDER engine (sandboxed page process)
Exec=$D/omni-ui run web %u
Icon=omni-web
Terminal=false
Categories=Network;WebBrowser;
MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;
StartupWMClass=omni-ui
Keywords=browser;web;internet;
S
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
echo "Omni Web installed: app menu → Omni Web, or: omni-web [url]"
