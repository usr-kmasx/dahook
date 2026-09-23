# Browser — apenas atalhos chromium (modo web do dahook)
Implementados no binário único (`main.cpp:buildShortcuts`, grupo webKeys):
- Ctrl+L foca URL | F5 / Ctrl+R recarrega | Ctrl+Shift+R hard reload
- Alt+Left/Right voltar/avançar | Esc parar
- Ctrl+F buscar | Ctrl+Plus/Minus/0 zoom / reset
Motor: QtWebEngine (Chromium, mesmo do `../browser`/qutebrowser).
Fotos OK; vídeo/áudio dependem dos codecs do Qt do sistema (H.264/AAC e
Widevine podem faltar — mesmo limite da base qutebrowser).
NÃO há abas múltiplas no MVP (Ctrl+T/W deliberadamente fora).
