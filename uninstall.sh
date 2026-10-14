#!/usr/bin/env bash
# Remove a Ferramenta de Captura. Os atalhos do COSMIC não são alterados:
# restaure ~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom.bak-*
# ou remova-os em Configurações › Teclado › Atalhos personalizados.
set -euo pipefail
PREFIX="${PREFIX:-$HOME/.local}"
rm -fv "$PREFIX/bin/recorte" \
       "$PREFIX/share/applications/io.github.recorte.Recorte.desktop" \
       "$PREFIX/share/icons/hicolor/scalable/apps/io.github.recorte.Recorte.svg"
rm -rfv "${XDG_DATA_HOME:-$HOME/.local/share}/recorte"
echo "Configurações mantidas em ~/.config/recorte (apague se quiser)."
