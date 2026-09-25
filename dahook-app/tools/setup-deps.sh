#!/usr/bin/env bash
# setup-deps.sh — instala as dependências DE SISTEMA do dahook
# (o `cargo build` já baixa os crates Rust sozinho).
#
# Detecta a base via /etc/os-release e usa o gerenciador nativo:
#   Arch/CachyOS/Manjaro/EndeavourOS -> pacman
#   Debian/Ubuntu/Mint/Pop!_OS       -> apt
#   Fedora/RHEL-like                 -> dnf
# Uso: ./tools/setup-deps.sh [--dry-run]
set -eu

DRY_RUN="${1:-}"
run() {
  if [ "$DRY_RUN" = "--dry-run" ]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

if [ ! -f /etc/os-release ]; then
  echo "setup-deps: sem /etc/os-release; instale manualmente (ver README)." >&2
  exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
ID_LIKE="${ID_LIKE:-}"

FAMILY=""
case "$ID" in
arch | cachyos | manjaro | endeavouros | garuda) FAMILY=arch ;;
debian | ubuntu | linuxmint | pop | zorin | elementary) FAMILY=debian ;;
fedora | rhel | centos | rocky | almalinux) FAMILY=fedora ;;
opensuse-tumbleweed | opensuse-leap | sles) FAMILY=suse ;;
*)
  case "$ID_LIKE" in
  *arch*) FAMILY=arch ;;
  *debian* | *ubuntu*) FAMILY=debian ;;
  *fedora* | *rhel*) FAMILY=fedora ;;
  *suse*) FAMILY=suse ;;
  esac
  ;;
esac

if [ -z "$FAMILY" ]; then
  echo "setup-deps: base '$ID' não mapeada. Pacotes (nomes Arch):" >&2
  echo "  gtk4 libadwaita vte4 webkitgtk-6.0 gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav" >&2
  exit 1
fi

echo "setup-deps: base detectada: $ID (familia $FAMILY)"

case "$FAMILY" in
arch)
  run sudo pacman -S --needed --noconfirm \
    gtk4 libadwaita vte4 webkitgtk-6.0 \
    gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav \
    yt-dlp
  ;;
debian)
  run sudo apt update
  run sudo apt install -y \
    libgtk-4-dev libadwaita-1-dev libvte-2.91-gtk4-dev libwebkitgtk-6.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
    yt-dlp
  ;;
fedora)
  run sudo dnf install -y \
    gtk4-devel libadwaita-devel vte291-gtk4-devel webkitgtk6.0-devel \
    gstreamer1-plugins-base gstreamer1-plugins-good \
    gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free \
    yt-dlp
  echo "NOTA Fedora: para H.264/AAC completos, adicione RPM Fusion (gstreamer1-libav, *-nonfree)."
  ;;
suse)
  # Nomes -devel variam entre releases: resolve via capability pkgconfig,
  # que o zypper traduz para o pacote certo em qualquer versão.
  echo "setup-deps: openSUSE é best-effort (sem teste ao vivo aqui):" >&2
  run sudo zypper install -y \
    'pkgconfig(gtk4)' 'pkgconfig(libadwaita-1)' \
    'pkgconfig(vte-2.91-gtk4)' 'pkgconfig(webkitgtk-6.0)' \
    gstreamer-plugins-base gstreamer-plugins-good \
    gstreamer-plugins-bad gstreamer-plugins-ugly gstreamer-plugins-libav \
    yt-dlp
  ;;
esac

echo "setup-deps: verificando pkg-config..."
for pc in gtk4 vte-2.91-gtk4 webkitgtk-6.0; do
  if pkg-config --exists "$pc"; then
    echo "  ok $pc $(pkg-config --modversion "$pc")"
  else
    echo "  FALTA $pc" >&2
  fi
done

if ! command -v cargo >/dev/null; then
  echo "setup-deps: 'cargo' não encontrado — instale via https://rustup.rs" >&2
fi
echo "setup-deps: pronto. Rode: cargo build"
