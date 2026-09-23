#!/usr/bin/env bash
# dahook deps — check/install das dependencias de BUILD do dahook-qt.
# Uso: tools/deps.sh check | install
# - check: só verifica, sai 0 se tudo ok, 1 listando o que falta.
# - install: tenta instalar o que falta (pacman/apt/dnf, só com sudo sem senha);
#   sem root, imprime o comando exato e sai 1 (sem fingir que instalou).
set -u
missing=()

have() { command -v "$1" >/dev/null 2>&1; }

check_prog() { # check_prog <bin> <pacote-pacman> <pacote-apt> <pacote-dnf>
  if have "$1"; then echo "OK   prog $1"; else echo "FALTA prog $1"; missing+=("$1|$2|$3|$4"); fi
}

check_pc() { # check_pc <modulo.pc> <pacote-pacman> <pacote-apt> <pacote-dnf>
  if pkg-config --exists "$1" 2>/dev/null; then
    echo "OK   lib $1 ($(pkg-config --modversion "$1"))"
  else
    echo "FALTA lib $1"; missing+=("$1|$2|$3|$4")
  fi
}

do_check() {
  echo "== dahook deps check =="
  check_prog cmake "cmake" "cmake" "cmake"
  check_prog g++ "gcc" "g++" "gcc-c++"
  if have ninja || have make; then echo "OK   build (ninja/make)"; else echo "FALTA build"; missing+=("ninja|ninja-build|ninja-build"); fi
  check_prog pkg-config "pkgconf" "pkg-config" "pkgconf-pkg-config"
  check_prog qmake6 "qt6-base" "qt6-base-dev" "qt6-qtbase-devel"
  check_pc Qt6Widgets "qt6-base" "qt6-base-dev" "qt6-qtbase-devel"
  check_pc Qt6WebEngineWidgets "qt6-webengine" "qtwebengine6-dev" "qt6-qtwebengine-devel"
  check_prog python3 "python" "python3" "python3"
  if [ "${#missing[@]}" -eq 0 ]; then echo "== tudo presente =="; return 0; fi
  echo "== faltando: ${#missing[@]} =="; return 1
}

pkg_manager() {
  if have pacman; then echo pacman; elif have apt; then echo apt; elif have dnf; then echo dnf; else echo none; fi
}

can_root() { sudo -n true 2>/dev/null; }

do_install() {
  do_check && return 0
  mgr=$(pkg_manager)
  [ "$mgr" = "none" ] && { echo "ERRO: nenhum gerenciador suportado (pacman/apt/dnf)."; return 1; }
  if ! can_root; then
    echo "ERRO: sem root (sudo pede senha). Rode manualmente:"
    case "$mgr" in
      pacman) echo "  sudo pacman -S --needed cmake gcc ninja pkgconf qt6-base qt6-webengine python" ;;
      apt) echo "  sudo apt install cmake g++ ninja-build pkg-config qt6-base-dev qtwebengine6-dev python3" ;;
      dnf) echo "  sudo dnf install cmake gcc-c++ ninja-build pkgconf-pkg-config qt6-qtbase-devel qt6-qtwebengine-devel python3" ;;
    esac
    echo "Qt via user-space (sem root, ~1GB, futuro): aqtinstall (ainda nao suportado aqui)."
    return 1
  fi
  echo "== instalando com $mgr (root ok) =="
  pkgs=""
  for m in "${missing[@]}"; do
    IFS='|' read -r _ pp pa pd <<<"$m"
    case "$mgr" in pacman) pkgs="$pkgs $pp";; apt) pkgs="$pkgs $pa";; dnf) pkgs="$pkgs $pd";; esac
  done
  # shellcheck disable=SC2086
  case "$mgr" in
    pacman) sudo -n pacman -S --needed --noconfirm $pkgs ;;
    apt) sudo -n apt update && sudo -n apt install -y $pkgs ;;
    dnf) sudo -n dnf install -y $pkgs ;;
  esac || return 1
  missing=()
  do_check
}

case "${1:-check}" in
  check) do_check ;;
  install) do_install ;;
  *) echo "uso: $0 check|install"; exit 2 ;;
esac
