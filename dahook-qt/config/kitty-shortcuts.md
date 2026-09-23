# Terminal — tabela manual equivalente kitty (modo terminal do dahook)
Digitação direta no terminal (sem caixa de entrada): toda tecla vai ao pty e
o readline do bash faz edição, histórico (seta pra cima, persistente),
Tab-completion e Ctrl-* nativos. Cursor em bloco desenhado no fim.
Implementado em `main.cpp:termKeyBytes` (testado no selftest):
- Enter `\r` | Backspace `\x7f` | Delete, Tab, Esc, setas, Home/End, PgUp/PgDn
- Ctrl+A/C/D/E/K/L/U/W/Z como controle (`\x01 \x03 \x04 ...`) — Ctrl+C
  interrompe, Ctrl+D sai (reinicia shell), Ctrl+L limpa via `clear`
- Ctrl+Shift+C copiar | Ctrl+Shift+V colar (clipboard -> shell)
- Ctrl+Shift+Plus/Minus zoom fonte
Shell: bash interativo via forkpty, cwd=$HOME (prompt mostra ~).
Limites declarados: redesenhos do readline (Tab, edição no meio da linha)
podem duplicar linhas no visor; 80x24 fixo; sem cores ANSI.
Origem da tabela: `../kitty/kitty/options/definition.py`.
