#!/usr/bin/env bash
# Removes Snipping Tool. COSMIC shortcuts are left untouched: restore
# ~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom.bak-*
# or remove them in Settings › Keyboard › Custom shortcuts.
set -euo pipefail
PREFIX="${PREFIX:-$HOME/.local}"
rm -fv "$PREFIX/bin/recorte" \
       "$PREFIX/share/applications/io.github.recorte.Recorte.desktop" \
       "$PREFIX/share/icons/hicolor/scalable/apps/io.github.recorte.Recorte.svg"
rm -rfv "${XDG_DATA_HOME:-$HOME/.local/share}/recorte"
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
    pt*) echo "Configurações mantidas em ~/.config/recorte (apague se quiser)." ;;
    *) echo "Settings kept in ~/.config/recorte (delete it if you want)." ;;
esac
