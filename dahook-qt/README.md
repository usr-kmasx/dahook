# dahook — UM binário, UMA janela

Terminal (bash, nasce em `~`) que vira browser EMBUTIDO na mesma janela.
X no canto direito da barra do app volta ao terminal.

## O que NÃO é (limites, sem contorno silencioso)
- kitty e qutebrowser NÃO estão linkados aqui: são C/Python/Go e Python/Qt,
  não viram objeto Qt. O terminal replica a tabela kitty e o browser usa o
  MESMO motor (QtWebEngine/Chromium). Tabelas em `config/`.
- `../browser`/qutebrowser não executa nesta máquina (sem PyQt6) nem embute
  como widget (processo/janela próprios). Equivalente embutido = QWebEngineView.
- Terminal = bash interativo via forkpty (tty real: rcfile, `~`, job control),
  80x24 fixo, cores ANSI removidas, digitação direta com pass-through
  (readline faz edição/histórico/Tab). `exit` reinicia o shell.
- O app responde queries de terminal (DA VT220, teclado kitty, XTVERSION,
  OSC 11 com a cor real, XTGETTCAP 0+r, DSR aproximado): fish e outros shells
  não travam no boot. Redesenhos com movimento de cursor (fish) podem
  duplicar trechos no visor — modelo de tela (screen grid) é trabalho futuro.

## Build / teste
```bash
cd dahook-qt
cmake -S . -B build -G Ninja   # o configure verifica as deps (tools/deps.sh);
cmake --build build            # use -DAHOOK_AUTO_DEPS=OFF para gerenciar na mão
./build/dahook --selftest   # sem GUI: testa trigger, home, rcfile
./build/dahook              # GUI: terminal em ~, `dahook` vira browser, X volta
```
Dependências de build (cmake, g++, ninja, pkgconf, qt6-base, qt6-webengine,
python3): `tools/deps.sh check` verifica; `tools/deps.sh install` instala o
que faltar (pacman/apt/dnf, só com sudo sem senha — sem root, imprime o
comando exato e falha em vez de fingir). Qt user-space sem root (~1GB via
aqtinstall) ainda não suportado.
Perfil browser isolado e arquivos de sinalização ficam em `$XDG_RUNTIME_DIR/dahook`
(fallback `/tmp/dahook`) — por usuário, sem colisão em máquina multiusuário.
