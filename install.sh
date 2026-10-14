#!/usr/bin/env bash
# Instala a Ferramenta de Captura (recorte) para o usuário atual.
#   ./install.sh               compila, instala e liga Print / Super+Shift+S
#   ./install.sh --sem-atalhos compila e instala sem mexer nos atalhos
# No pacote da release (recorte-*-x86_64.tar.gz) o binário já vem compilado.
set -euo pipefail
cd "$(dirname "$0")"

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/recorte"
APPS="$PREFIX/share/applications"
ICON="$PREFIX/share/icons/hicolor/scalable/apps"

# No pacote da release o binário já vem pronto; no código-fonte, compila.
if [[ -x ./recorte ]]; then
    SRC=./recorte
else
    echo "• Compilando (release)…"
    cargo build --release
    SRC=target/release/recorte
fi

echo "• Instalando em $PREFIX"
install -Dm755 "$SRC" "$BIN"
install -Dm644 data/io.github.recorte.Recorte.svg "$ICON/io.github.recorte.Recorte.svg"
mkdir -p "$APPS"
sed "s|@BIN@|$BIN|g" data/io.github.recorte.Recorte.desktop.in > "$APPS/io.github.recorte.Recorte.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true

if [[ "${1:-}" != "--sem-atalhos" ]]; then
    echo "• Configurando atalhos do COSMIC (o arquivo anterior fica salvo como custom.bak-*)"
    "$BIN" configurar-atalhos
fi
echo "Pronto. Pressione Print Screen ou Super + Shift + S."
