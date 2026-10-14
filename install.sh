#!/usr/bin/env bash
# Installs Snipping Tool (recorte) for the current user.
#   ./install.sh                 builds, installs and binds Print / Super+Shift+S
#   ./install.sh --no-shortcuts  builds and installs without touching shortcuts
# The release package (recorte-*-x86_64.tar.gz) ships a prebuilt binary.
set -euo pipefail
cd "$(dirname "$0")"

# Messages follow the system language (Portuguese or English).
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
    pt*) PT=1 ;;
    *) PT= ;;
esac
say() { if [[ -n "$PT" ]]; then echo "$2"; else echo "$1"; fi; }

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/recorte"
APPS="$PREFIX/share/applications"
ICON="$PREFIX/share/icons/hicolor/scalable/apps"

# Release package: prebuilt binary; source tree: build it.
if [[ -x ./recorte ]]; then
    SRC=./recorte
else
    say "• Building (release)…" "• Compilando (release)…"
    cargo build --release
    SRC=target/release/recorte
fi

say "• Installing to $PREFIX" "• Instalando em $PREFIX"
install -Dm755 "$SRC" "$BIN"
install -Dm644 data/io.github.recorte.Recorte.svg "$ICON/io.github.recorte.Recorte.svg"
mkdir -p "$APPS"
sed "s|@BIN@|$BIN|g" data/io.github.recorte.Recorte.desktop.in > "$APPS/io.github.recorte.Recorte.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true

if [[ "${1:-}" != "--no-shortcuts" && "${1:-}" != "--sem-atalhos" ]]; then
    say "• Setting up COSMIC shortcuts (the previous file is kept as custom.bak-*)" \
        "• Configurando atalhos do COSMIC (o arquivo anterior fica salvo como custom.bak-*)"
    "$BIN" setup-shortcuts
fi
say "Done. Press Print Screen or Super + Shift + S." "Pronto. Pressione Print Screen ou Super + Shift + S."
