# Atalhos Chromium — modo web do dahook (apenas atalhos do chromium)
# O modo web usa chromium REAL com perfil isolado, entao os atalhos sao nativos.
# Lista manual para referencia e para a futura implementacao pura-Rust.
# Fonte: chrome://settings/keyboardShortcuts + help do chromium.

## Abas / janelas
- Ctrl+T nova aba | Ctrl+W fecha aba | Ctrl+Shift+T reabre | Ctrl+Tab proxima | Ctrl+Shift+Tab anterior
- Ctrl+1..8 vai p/ aba N | Ctrl+9 ultima aba
- Ctrl+N nova janela | Ctrl+Shift+N anonima (desabilitada no perfil dahook) | Alt+F4 fecha

## Navegacao
- Alt+Left / Alt+Right voltar/avancar | F5 / Ctrl+R recarrega | Ctrl+Shift+R hard reload
- Ctrl+L / Alt+D foca barra URL | Ctrl+K pesquisa | Esc para pesquisa
- Espaco / Shift+Espaco rola | Home / End topo/fim | Ctrl+F buscar na pagina

## Zoom / midia
- Ctrl+Plus / Ctrl+Minus / Ctrl+0 zoom | Ctrl+Shift+I devtools (opcional desabilitar)
- M mute aba (quando hover) | F11 fullscreen | Ctrl+P imprimir | Ctrl+S salvar | Ctrl+D favoritar

## Regra do dahook
No modo web, SO esses atalhos valem. Atalhos do kitty ficam suspensos ate clicar o X.
O X (overlay canto superior direito) = `dahook back` = mata browser, restaura kitty.
