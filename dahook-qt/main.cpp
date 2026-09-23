// dahook — UM binário só, UMA janela só.
// Página 0: terminal (bash INTERATIVO via forkpty, cwd=$HOME, prompt mostra ~).
// Página 1: browser embutido (QWebEngineView, mesmo motor Chromium do qutebrowser).
// `dahook [url]` no shell troca para o browser DENTRO da janela.
// X no canto direito do app volta ao terminal (não é botão flutuante).
//
// Limites honestos:
// - kitty/qutebrowser NÃO linkam aqui (C/Python/Go, Python/Qt). Terminal replica
//   a tabela kitty; browser usa o mesmo motor (QtWebEngine).
// - forkpty 80x24 fixo; cores ANSI removidas na v2; Ctrl+C/D enviados ao shell;
//   edição é na caixa de entrada (digitar direto no log = fase 3).

#include <errno.h>
#include <pty.h>
#include <signal.h>
#include <sys/select.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#include <QApplication>
#include <QClipboard>
#include <QDir>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QGuiApplication>
#include <QInputDialog>
#include <QLabel>
#include <QLineEdit>
#include <QMainWindow>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollBar>
#include <QShortcut>
#include <QSocketNotifier>
#include <QStackedWidget>
#include <QTextStream>
#include <QTimer>
#include <QToolBar>
#include <QUrl>
#include <QVBoxLayout>
#include <QWebEnginePage>
#include <QWebEngineProfile>
#include <QWebEngineView>

namespace {
// Por usuário (XDG_RUNTIME_DIR, fallback /tmp): sem crosstalk em máquina
// multiusuário — publicar exige isso, /tmp puro colidiria entre usuários.
QString dahookRunDir() {
    QString base = QString::fromLocal8Bit(qgetenv("XDG_RUNTIME_DIR"));
    if (base.isEmpty()) base = "/tmp";
    return base + "/dahook";
}
inline QString kTrigger() { return dahookRunDir() + "/trigger"; }
inline QString kReady() { return dahookRunDir() + "/ready"; }
inline QString kMode() { return dahookRunDir() + "/mode"; }
inline QString kProfile() { return dahookRunDir() + "/webprofile"; }
inline QString kRcFile() { return dahookRunDir() + "/bashrc"; }
const char *kHomeUrl = "https://duckduckgo.com";

void writeFile(const QString &path, const QString &content) {
    QDir().mkpath(QFileInfo(path).path());
    QFile f(path);
    if (f.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        f.write(content.toUtf8());
    }
}

QString readTrigger() {
    QFile f(kTrigger());
    if (!f.exists()) return QString(); // nulo = sem trigger
    if (!f.open(QIODevice::ReadOnly)) return QString();
    QString s = QString::fromUtf8(f.readAll()).trimmed();
    f.close();
    QFile::remove(kTrigger());
    return s;
}

QString bashRcContent() {
    return QString(
               "# dahook bashrc (gerado pelo binario dahook)\n"
               "if [ -f /etc/bash.bashrc ]; then source /etc/bash.bashrc; fi\n"
               "if [ -f ~/.bashrc ]; then source ~/.bashrc; fi\n"
               "PS1='\\u@\\h \\W\\$ '\n"
               "dahook() {\n"
               "  local url=\"${1:-__HOME__}\"\n"
               "  echo \"$url\" > __TRIGGER__\n"
               "  echo \"[dahook] carregando browser dentro do app: $url\"\n"
               "}\n"
               "command -v dahook >/dev/null && touch __READY__\n")
        .replace("__HOME__", kHomeUrl)
        .replace("__TRIGGER__", kTrigger())
        .replace("__READY__", kReady());
}

// --- filtro ANSI: remove escapes, sinaliza clear-screen. Bytes não-ASCII passam crus (UTF-8 preservado).
struct AnsiResult {
    QByteArray bytes;
    bool clear = false;
};
AnsiResult filterAnsi(const QByteArray &in) {
    AnsiResult r;
    r.bytes.reserve(in.size());
    const int n = in.size();
    int i = 0;
    auto u = [&](int k) -> unsigned char { return static_cast<unsigned char>(in[k]); };
    while (i < n) {
        unsigned char c = u(i);
        if (c == 0x1b) { // ESC
            if (i + 1 >= n) break;
            unsigned char d = u(i + 1);
            if (d == '[') { // CSI ... final
                int j = i + 2;
                while (j < n && !(u(j) >= '@' && u(j) <= '~')) j++;
                if (j >= n) break;
                if (u(j) == 'J' && in.mid(i + 2, j - (i + 2)).contains('2')) r.clear = true; // ED 2J
                i = j + 1;
                continue;
            }
            if (d == ']') { // OSC ... BEL ou ESC backslash
                int j = i + 2;
                while (j < n) {
                    if (u(j) == 0x07) { j++; break; }
                    if (u(j) == 0x1b && j + 1 < n && u(j + 1) == '\\') { j += 2; break; }
                    j++;
                }
                i = j;
                continue;
            }
            if (d == 'P' || d == 'X' || d == '^' || d == '_') { // DCS-like ... ESC backslash
                int j = i + 2;
                while (j + 1 < n && !(u(j) == 0x1b && u(j + 1) == '\\')) j++;
                i = qMin(j + 2, n);
                continue;
            }
            if (d == 'c') { r.clear = true; i += 2; continue; } // RIS reset
            if (d == '(' || d == ')' || d == '#' || d == '%') { i = qMin(i + 3, n); continue; }
            i += 2; // demais escapes de 2 bytes (M, =, >, 7, 8...)
            continue;
        }
        if (c == '\r') { // passa cru: feedLines decide (CR sozinho sobrescreve, CR+LF quebra)
            r.bytes += '\r';
            i++;
            continue;
        }
        if (c == '\a' || c == '\b' || c == '\v' || c == '\f') { i++; continue; }
        r.bytes += (char)c; // \t \n imprimiveis e UTF-8 multibyte passam crus
        i++;
    }
    return r;
}

// Mapeia tecla Qt -> bytes pro pty. Vazio = ignora (atalho do app).
// Pass-through: readline do bash faz edição, histórico, Tab e Ctrl-* nativo.
QByteArray termKeyBytes(int k, Qt::KeyboardModifiers m, const QString &text) {
    const bool ctrl = m.testFlag(Qt::ControlModifier);
    const bool shift = m.testFlag(Qt::ShiftModifier);
    const bool alt = m.testFlag(Qt::AltModifier);
    const bool meta = m.testFlag(Qt::MetaModifier);
    if (k == Qt::Key_Shift || k == Qt::Key_Control || k == Qt::Key_Alt || k == Qt::Key_Meta) return {};
    if (ctrl && shift && !alt && !meta) return {}; // Ctrl+Shift+* = atalhos do app
    if (ctrl && !alt && !meta && !shift) {
        switch (k) {
            case Qt::Key_A: return "\x01";
            case Qt::Key_C: return "\x03";
            case Qt::Key_D: return "\x04";
            case Qt::Key_E: return "\x05";
            case Qt::Key_K: return "\x0b";
            case Qt::Key_L: return "\x0c";
            case Qt::Key_U: return "\x15";
            case Qt::Key_W: return "\x17";
            case Qt::Key_Z: return "\x1a";
            default: return {};
        }
    }
    if (ctrl || alt || meta) return {};
    switch (k) {
        case Qt::Key_Return:
        case Qt::Key_Enter: return "\r";
        case Qt::Key_Backspace: return "\x7f";
        case Qt::Key_Delete: return "\x1b[3~";
        case Qt::Key_Tab: return "\t";
        case Qt::Key_Escape: return "\x1b";
        case Qt::Key_Up: return "\x1b[A";
        case Qt::Key_Down: return "\x1b[B";
        case Qt::Key_Right: return "\x1b[C";
        case Qt::Key_Left: return "\x1b[D";
        case Qt::Key_Home: return "\x1b[H";
        case Qt::Key_End: return "\x1b[F";
        case Qt::Key_PageUp: return "\x1b[5~";
        case Qt::Key_PageDown: return "\x1b[6~";
        default: break;
    }
    if (!text.isEmpty()) return text.toLocal8Bit();
    return {};
}

// Modelo de linha com CR real: \r arma sobrescrita (preguiçoso), texto
// subsequente apaga a linha pendente, \n confirma a linha mesmo após \r.
// Assim `\r\r\n` depois do comando = UMA quebra (sem linha fantasma),
// `50%\r60%` vira `60%`, e `\n\n` intencional continua em branco.
struct LineBuf {
    QStringList done;
    QString pending;
    bool drop = false;
};
void feedLines(LineBuf &s, const QString &t) {
    for (QChar ch : t) {
        if (ch == '\r') {
            s.drop = true;
            continue;
        }
        if (ch == '\n') {
            s.drop = false;
            s.done << s.pending;
            s.pending.clear();
            continue;
        }
        if (s.drop) {
            s.pending.clear();
            s.drop = false;
        }
        s.pending += ch;
    }
}

// --- Respostas de terminal (queries de capabilities) ---
// fish/zsh interrogam o terminal no boot (DA, teclado kitty, XTVERSION, cor de
// fundo, terminfo, posição do cursor) e TRAVAM ~10s sem resposta. Terminal de
// verdade responde; respondemos conservador (nível VT220) sem mentir recursos.
static bool isHexByte(unsigned char c) {
    return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}
struct QMatch {
    int len = 0; // >0 = query completa (responder + consumir len bytes)
    bool partial = false; // prefixo viável, faltam bytes
    QByteArray reply;
};
// Casa UMA query em b[pos]. row/col = posição aproximada do cursor (p/ DSR).
QMatch matchQuery(const QByteArray &b, int pos, int row, int col, const QString &bg) {
    QMatch m;
    const int n = b.size();
    auto U = [&](int k) -> unsigned char { return (k < n) ? (unsigned char)b[k] : 0; };
    if (U(pos) != 0x1b) return m;
    if (pos + 1 >= n) {
        m.partial = true;
        return m;
    }
    unsigned char d = U(pos + 1);
    if (d == '[') {
        int j = pos + 2;
        if (j < n && U(j) == '?') {
            if (j + 1 >= n) {
                m.partial = true;
                return m;
            }
            if (U(j + 1) == 'u') { // teclado kitty: sem enhancement
                m.len = j + 2 - pos;
                m.reply = "\x1b[?0u";
                return m;
            }
            return m;
        }
        if (j < n && U(j) == '>') { // XTVERSION
            int k = j + 1;
            while (k < n && U(k) >= '0' && U(k) <= '9') k++;
            if (k >= n) {
                m.partial = true;
                return m;
            }
            if (U(k) == 'q') {
                m.len = k + 1 - pos;
                m.reply = "\x1bP>|dahook\x1b\\";
                return m;
            }
            return m;
        }
        if (j < n && U(j) == '6') { // DSR (posição do cursor)
            if (j + 1 >= n) {
                m.partial = true;
                return m;
            }
            if (U(j + 1) == 'n') {
                m.len = j + 2 - pos;
                m.reply = "\x1b[" + QByteArray::number(row) + ";" + QByteArray::number(col) + "R";
                return m;
            }
        }
        { // DA primário genérico: ESC [ [0-9;]* c
            int k = j;
            while (k < n && ((U(k) >= '0' && U(k) <= '9') || U(k) == ';')) k++;
            if (k >= n) {
                m.partial = true;
                return m;
            }
            if (U(k) == 'c') {
                m.len = k + 1 - pos;
                m.reply = "\x1b[?62;c"; // VT220: conservador
                return m;
            }
            return m;
        }
    }
    if (d == ']') { // OSC 11 (cor de fundo): ESC ] 1 1 ; ? (ST|BEL)
        const char *pre = "]11;?";
        int k = pos + 1;
        for (int t = 0; pre[t]; t++, k++) {
            if (k >= n) {
                m.partial = true;
                return m;
            }
            if (U(k) != (unsigned char)pre[t]) return m;
        }
        if (k >= n) {
            m.partial = true;
            return m;
        }
        if (U(k) == 0x07) {
            m.len = k + 1 - pos;
        } else if (U(k) == 0x1b) {
            if (k + 1 >= n) {
                m.partial = true;
                return m;
            }
            if (U(k + 1) != '\\') return m;
            m.len = k + 2 - pos;
        } else {
            return m;
        }
        if (bg.isEmpty()) return QMatch(); // sem cor conhecida: não responde
        m.reply = "\x1b]11;" + bg.toLatin1() + "\x1b\\";
        return m;
    }
    if (d == 'P') { // XTGETTCAP: ESC P + q hex+ (ST|BEL) -> 0+r (sem caps anunciadas)
        int k = pos + 2;
        if (k >= n) {
            m.partial = true;
            return m;
        }
        if (U(k) != '+') return m;
        if (++k >= n) {
            m.partial = true;
            return m;
        }
        if (U(k) != 'q') return m;
        if (++k >= n) {
            m.partial = true;
            return m;
        }
        int h0 = k;
        while (k < n && isHexByte(U(k))) k++;
        if (k >= n) {
            m.partial = true;
            return m;
        }
        if (k == h0) return m;
        if (k >= n) {
            m.partial = true;
            return m;
        }
        if (U(k) == 0x07) {
            m.len = k + 1 - pos;
        } else if (U(k) == 0x1b) {
            if (k + 1 >= n) {
                m.partial = true;
                return m;
            }
            if (U(k + 1) != '\\') return m;
            m.len = k + 2 - pos;
        } else {
            return m;
        }
        m.reply = "\x1bP0+r\x1b\\";
        return m;
    }
    return m;
}
// Varre buf respondendo queries completas. carry guarda sufixo parcial entre
// leituras (query partida em 2 pacotes). row/col/bg p/ DSR e OSC 11.
QByteArray scanQueries(QByteArray &carry, const QByteArray &fresh, int row, int col, const QString &bg) {
    QByteArray reply;
    QByteArray buf = carry + fresh;
    carry.clear();
    const int base = buf.size() - fresh.size();
    const int n = buf.size();
    int pos = 0;
    while (pos < n) {
        if ((unsigned char)buf[pos] != 0x1b) {
            pos++;
            continue;
        }
        QMatch m = matchQuery(buf, pos, row, col, bg);
        if (m.len > 0) {
            if (pos + m.len > base) reply += m.reply; // só o que usa bytes novos
            pos += m.len;
            continue;
        }
        pos++;
    }
    int esc = buf.lastIndexOf('\x1b'); // sufixo parcial vira carry (cap 64)
    if (esc >= 0 && n - esc <= 64) {
        QMatch m = matchQuery(buf, esc, row, col, bg);
        if (m.len == 0 && m.partial) carry = buf.mid(esc);
    }
    return reply;
}
// Modelo aproximado do cursor (p/ DSR): escapes não contam coluna.
void countCursor(const QByteArray &b, int &row, int &col) {
    const int n = b.size();
    int i = 0;
    auto U = [&](int k) -> unsigned char { return (unsigned char)b[k]; };
    while (i < n) {
        unsigned char c = U(i);
        if (c == 0x1b) {
            if (i + 1 >= n) break;
            unsigned char d = U(i + 1);
            if (d == '[') {
                int j = i + 2;
                while (j < n && !(U(j) >= '@' && U(j) <= '~')) j++;
                i = (j >= n) ? n : j + 1;
                continue;
            }
            if (d == ']') {
                int j = i + 2;
                while (j < n) {
                    if (U(j) == 0x07) { j++; break; }
                    if (U(j) == 0x1b && j + 1 < n && U(j + 1) == '\\') { j += 2; break; }
                    j++;
                }
                i = j;
                continue;
            }
            if (d == 'P' || d == 'X' || d == '^' || d == '_') {
                int j = i + 2;
                while (j + 1 < n && !(U(j) == 0x1b && U(j + 1) == '\\')) j++;
                i = qMin(j + 2, n);
                continue;
            }
            if (d == '(' || d == ')' || d == '#' || d == '%') {
                i = qMin(i + 3, n);
                continue;
            }
            i += 2;
            continue;
        }
        if (c == '\n') {
            row++;
            col = 1;
            i++;
            continue;
        }
        if (c == '\r') {
            col = 1;
            i++;
            continue;
        }
        if (c == '\t') {
            col = ((col - 1) / 8 + 1) * 8 + 1;
            i++;
            continue;
        }
        if (c < 0x20 || c == 0x7f) {
            i++;
            continue;
        }
        if (c < 0x80 || c >= 0xC0) col++; // 1 por sequência UTF-8
        i++;
    }
    if (row < 1) row = 1;
    if (col < 1) col = 1;
}

// Sobe bash INTERATIVO de verdade num pty (igual ao teste) e devolve a saída.
bool ptyShellCheck(QTextStream &out) {
    writeFile(kRcFile(), bashRcContent());
    out << "PTY-TEST spawn\n";
    out.flush();
    int master = -1;
    pid_t pid = forkpty(&master, nullptr, nullptr, nullptr);
    if (pid < 0) {
        out << "pty-spawn falhou: " << strerror(errno) << "\n";
        return false;
    }
    if (pid == 0) {
        const QByteArray rc = kRcFile().toLocal8Bit();
        execlp("bash", "bash", "--rcfile", rc.constData(), (char *)nullptr);
        _exit(127);
    }
    const char *cmds = "echo PTY_MARKER_OK\ntype -t dahook\nexit\n"; // -t imprime "function" em qualquer locale
    ::write(master, cmds, strlen(cmds));
    QByteArray all;
    QElapsedTimer t;
    t.start();
    while (t.elapsed() < 8000) {
        fd_set rf;
        FD_ZERO(&rf);
        FD_SET(master, &rf);
        struct timeval tv{1, 0};
        int s = ::select(master + 1, &rf, nullptr, nullptr, &tv);
        if (s > 0 && FD_ISSET(master, &rf)) {
            char buf[4096];
            ssize_t k = ::read(master, buf, sizeof buf);
            if (k <= 0) break;
            all += QByteArray(buf, (int)k);
            LineBuf lb;
            feedLines(lb, QString::fromUtf8(filterAnsi(all).bytes));
            if (lb.done.join("\n").contains("PTY_MARKER_OK") && lb.done.contains("function")) break;
        }
    }
    ::close(master);
    int st = 0;
    ::waitpid(pid, &st, 0);
    QString txt = QString::fromUtf8(filterAnsi(all).bytes); // valida o pipeline real (filtro + shell)
    LineBuf lb;
    feedLines(lb, txt);
    QStringList lines = lb.done;
    bool ok = lines.join("\n").contains("PTY_MARKER_OK") && lines.contains("function");
    if (!ok) out << "saida-pty:\n" << txt << "\n";
    return ok;
}

// O rcfile sinaliza o ready sozinho, sem imprimir nada no shell.
// O teste exige: ready aparece E nada com "dahook" vaza no transcript.
bool ptyReadyCheck(QTextStream &out) {
    QFile::remove(kReady());
    writeFile(kRcFile(), bashRcContent());
    int master = -1;
    pid_t pid = forkpty(&master, nullptr, nullptr, nullptr);
    if (pid < 0) return false;
    if (pid == 0) {
        const QByteArray home = QDir::homePath().toLocal8Bit();
        ::chdir(home.constData()); // igual ao app: prompt mostra ~, sem "dahook" no caminho
        const QByteArray rc = kRcFile().toLocal8Bit();
        execlp("bash", "bash", "--rcfile", rc.constData(), (char *)nullptr);
        _exit(127);
    }
    QElapsedTimer t;
    t.start();
    bool seen = false;
    while (t.elapsed() < 6000 && !seen) {
        if (QFile::exists(kReady())) {
            seen = true;
            break;
        }
        ::usleep(100000);
    }
    // Drena o transcript e sai; nada sobre dahook pode ter sido impresso.
    QByteArray all;
    const char *x = "exit\n";
    ::write(master, x, strlen(x));
    QElapsedTimer t2;
    t2.start();
    while (t2.elapsed() < 3000) {
        fd_set rf;
        FD_ZERO(&rf);
        FD_SET(master, &rf);
        struct timeval tv{0, 200000};
        int s = ::select(master + 1, &rf, nullptr, nullptr, &tv);
        if (s > 0 && FD_ISSET(master, &rf)) {
            char buf[4096];
            ssize_t k = ::read(master, buf, sizeof buf);
            if (k <= 0) break;
            all += QByteArray(buf, (int)k);
        }
    }
    ::close(master);
    int st = 0;
    ::waitpid(pid, &st, 0);
    QFile::remove(kReady());
    QString txt = QString::fromUtf8(filterAnsi(all).bytes);
    bool ok = seen && !txt.contains("trigger") && !txt.contains("ready") && !txt.contains("DAH OOK");
    if (!ok) out << "ready-seen=" << seen << " transcript:\n" << txt << "\n";
    return ok;
}

int selftest() {
    QTextStream out(stdout);
    int fails = 0;
    auto check = [&](bool ok, const char *name) {
        out << (ok ? "PASS " : "FAIL ") << name << "\n";
        out.flush();
        if (!ok) fails++;
    };
    writeFile(kTrigger(), "https://example.com\n");
    check(readTrigger() == "https://example.com", "trigger-url");
    check(!QFile::exists(kTrigger()), "trigger-consumed");
    writeFile(kTrigger(), "   \n");
    QString t = readTrigger();
    check((t.isEmpty() ? QString(kHomeUrl) : t) == kHomeUrl, "trigger-empty-means-home");
    QString rc = bashRcContent();
    check(rc.contains("\\W"), "rcfile-shows-tilde");
    check(rc.contains(kHomeUrl), "rcfile-default-home");
    check(rc.contains("dahook()"), "rcfile-defines-dahook");
    check(rc.contains(kReady()), "rcfile-signals-ready");
    check(QDir::home().exists(), "home-exists");
    // filtro ANSI
    {
        AnsiResult a = filterAnsi("\x1b[32mVerde\x1b[0m\n");
        check(QString::fromUtf8(a.bytes) == "Verde\n" && !a.clear, "ansi-strip-color");
    }
    {
        AnsiResult a = filterAnsi("x\x1b[H\x1b[2Jy\n");
        check(a.clear && QString::fromUtf8(a.bytes) == "xy\n", "ansi-clear-screen");
    }
    {
        AnsiResult a = filterAnsi("a\r\nb\rc\n");
        check(QString::fromUtf8(a.bytes) == "a\r\nb\rc\n", "ansi-cr-passthrough"); // \r cru: feedLines decide
    }
    {
        AnsiResult a = filterAnsi("Área de trabalho\n"); // UTF-8 intacto
        check(QString::fromUtf8(a.bytes) == "Área de trabalho\n", "ansi-utf8");
    }
    // modelo de linha: o caso do print (Enter gera \r\r\n -> UMA quebra)
    {
        LineBuf s;
        feedLines(s, "user@host ~$ ls");
        feedLines(s, "\r\r\n");
        check(s.done.size() == 1 && s.done[0] == "user@host ~$ ls" && s.pending.isEmpty(), "lines-no-phantom-blank");
    }
    {
        LineBuf s;
        feedLines(s, "50%\r60%\n");
        check(s.done.size() == 1 && s.done[0] == "60%", "lines-cr-overwrite");
    }
    {
        LineBuf s;
        feedLines(s, "a\r\nb\rc\n");
        check(s.done.size() == 2 && s.done[0] == "a" && s.done[1] == "c", "lines-cr");
    }
    {
        LineBuf s;
        feedLines(s, "a\n\nb");
        check(s.done.size() == 2 && s.done[1].isEmpty() && s.pending == "b", "lines-blank-kept");
    }
    {
        LineBuf s;
        feedLines(s, "user@host ~$ ");
        check(s.done.isEmpty() && s.pending == "user@host ~$ ", "lines-prompt-pending");
    }
    // mapeamento tecla -> pty (digitação direta, sem caixa de entrada)
    check(termKeyBytes(Qt::Key_Return, Qt::NoModifier, "") == "\r", "key-enter");
    check(termKeyBytes(Qt::Key_Backspace, Qt::NoModifier, "") == "\x7f", "key-backspace");
    check(termKeyBytes(Qt::Key_Up, Qt::NoModifier, "") == "\x1b[A", "key-up-history");
    check(termKeyBytes(Qt::Key_C, Qt::ControlModifier, "") == "\x03", "key-ctrl-c");
    check(termKeyBytes(Qt::Key_C, Qt::ControlModifier | Qt::ShiftModifier, "").isEmpty(), "key-copy-reserved");
    check(termKeyBytes(Qt::Key_A, Qt::NoModifier, "a") == "a", "key-printable");
    check(!termKeyBytes(Qt::Key_unknown, Qt::NoModifier, QString::fromUtf8("ã")).isEmpty(), "key-utf8");
    // queries de terminal: fish trava ~10s sem resposta (print do usuário)
    {
        QByteArray c;
        check(scanQueries(c, "\x1b[0c", 1, 1, "") == "\x1b[?62;c", "q-da");
        check(scanQueries(c, "\x1b[c", 1, 1, "") == "\x1b[?62;c", "q-da-noparam");
        check(scanQueries(c, "\x1b[?u", 1, 1, "") == "\x1b[?0u", "q-kitty");
        check(scanQueries(c, "\x1b[>0q", 1, 1, "").contains("dahook"), "q-xtversion");
        check(scanQueries(c, "\x1b]11;?\x1b\\", 1, 1, "2a2a/2e2e/3333").contains("2a2a/2e2e/3333"),
              "q-osc11");
        check(scanQueries(c, "\x1bP+q1b1b\x1b\\", 1, 1, "") == "\x1bP0+r\x1b\\", "q-xtgettcap");
        check(scanQueries(c, "\x1b[6n", 24, 80, "") == "\x1b[24;80R", "q-dsr");
        check(scanQueries(c, "\x1b[32m", 1, 1, "").isEmpty(), "q-sgr-silent");
        check(scanQueries(c, "ls\n", 1, 1, "").isEmpty(), "q-text-silent");
    }
    { // query partida em 2 leituras
        QByteArray c;
        check(scanQueries(c, "\x1b[", 1, 1, "").isEmpty() && !c.isEmpty(), "q-split-1");
        check(scanQueries(c, "0c", 1, 1, "") == "\x1b[?62;c", "q-split-2");
    }
    { // contador aproximado p/ DSR (escapes não contam coluna)
        int r = 1, col = 1;
        countCursor("ab\ncd", r, col);
        check(r == 2 && col == 3, "cur-basic");
    }
    {
        int r = 1, col = 1;
        countCursor("\x1b[32mX", r, col);
        check(r == 1 && col == 2, "cur-skip-esc");
    }
    {
        int r = 5, col = 9;
        countCursor("a\rb", r, col);
        check(r == 5 && col == 2, "cur-cr");
    }
    // shell real no pty: rcfile carrega e dahook existe (pega o bug do print)
    out << "PTY-TEST start\n";
    out.flush();
    check(ptyShellCheck(out), "pty-bash-loads-rcfile-and-dahook");
    check(ptyReadyCheck(out), "pty-ready-silent");
    return fails == 0 ? 0 : 1;
}
} // namespace

// Bash interativo via forkpty: stdin/stdout do filho são um tty de verdade,
// então o bash lê o --rcfile, mostra PS1 com ~ e tem job control.
class PtyShell : public QObject {
    Q_OBJECT
public:
    explicit PtyShell(QObject *parent = nullptr) : QObject(parent) {}
    ~PtyShell() { stop(); }

    bool start() {
        stop();
        qcarry.clear();
        dsrRow = 1;
        dsrCol = 1;
        struct winsize ws{};
        ws.ws_row = 24;
        ws.ws_col = 80; // fixo na v2 (limite declarado)
        pid = forkpty(&master, nullptr, nullptr, &ws);
        if (pid < 0) return false;
        if (pid == 0) {
            const QByteArray home = QDir::homePath().toLocal8Bit();
            ::chdir(home.constData());
            ::setenv("TERM", "xterm-256color", 1);
            const QByteArray rc = kRcFile().toLocal8Bit();
            ::execlp("bash", "bash", "--rcfile", rc.constData(), (char *)nullptr);
            _exit(127);
        }
        timer.start();
        notifier = new QSocketNotifier(master, QSocketNotifier::Read, this);
        connect(notifier, &QSocketNotifier::activated, this, &PtyShell::onReady);
        return true;
    }

    void stop() {
        delete notifier;
        notifier = nullptr;
        if (master >= 0) {
            ::close(master);
            master = -1;
        }
        if (pid > 0) {
            ::kill(pid, SIGKILL);
            int st = 0;
            ::waitpid(pid, &st, WNOHANG);
            pid = -1;
        }
    }

    void writeLine(const QString &s) { writeRaw((s + "\n").toLocal8Bit()); }
    void writeRaw(const QByteArray &b) {
        if (master >= 0 && !b.isEmpty()) ::write(master, b.constData(), b.size());
    }
    void setBgRgb(const QString &s) { bgRgb = s; }
    qint64 uptimeMs() const { return timer.isValid() ? timer.elapsed() : -1; }

signals:
    void output(QByteArray data);
    void exited();

private slots:
    void onReady() {
        char buf[4096];
        ssize_t k = ::read(master, buf, sizeof buf);
        if (k > 0) {
            QByteArray fresh(buf, (int)k);
            QByteArray rep = scanQueries(qcarry, fresh, dsrRow, dsrCol, bgRgb);
            if (!rep.isEmpty()) ::write(master, rep.constData(), rep.size());
            countCursor(fresh, dsrRow, dsrCol);
            emit output(fresh);
        } else {
            emit exited(); // bash saiu (ex.: comando `exit`)
        }
    }

private:
    int master = -1;
    pid_t pid = -1;
    QSocketNotifier *notifier = nullptr;
    QElapsedTimer timer;
    QByteArray qcarry;
    int dsrRow = 1;
    int dsrCol = 1;
    QString bgRgb;
};

// Visor do terminal: só mostra saída, toda tecla vai pro pty.
// Cursor em bloco desenhado no fim (readline desenha o dele via escapes,
// que o filtro remove — o bloco local marca onde a digitação aparece).
class TermView : public QPlainTextEdit {
    Q_OBJECT
public:
    explicit TermView(QWidget *parent = nullptr) : QPlainTextEdit(parent) {
        setReadOnly(true);
        setFocusPolicy(Qt::StrongFocus);
        setUndoRedoEnabled(false);
        blink.setInterval(qMax(200, QApplication::cursorFlashTime() / 2));
        connect(&blink, &QTimer::timeout, this, [this]() {
            cursorOn = !cursorOn;
            viewport()->update();
        });
        blink.start();
    }
signals:
    void sendKeys(const QByteArray &data);
protected:
    void keyPressEvent(QKeyEvent *e) override {
        QByteArray b = termKeyBytes(e->key(), e->modifiers(), e->text());
        if (b.isEmpty()) {
            e->ignore();
            return;
        }
        emit sendKeys(b);
        e->accept();
    }
    void paintEvent(QPaintEvent *e) override {
        QPlainTextEdit::paintEvent(e);
        if (!hasFocus() || !cursorOn) return;
        QTextCursor c(document());
        c.movePosition(QTextCursor::End);
        QRect r = cursorRect(c);
        QFontMetrics fm(font());
        QPainter p(viewport());
        p.fillRect(QRect(r.topLeft(), QSize(qMax(3, fm.averageCharWidth()), r.height())),
                   palette().color(QPalette::Text));
    }
    void focusInEvent(QFocusEvent *e) override {
        QPlainTextEdit::focusInEvent(e);
        cursorOn = true;
        viewport()->update();
    }
private:
    QTimer blink;
    bool cursorOn = true;
};

class DahookWindow : public QMainWindow {
    Q_OBJECT
public:
    DahookWindow() {
        setWindowTitle("dahook");
        resize(1100, 750);

        toolbar = addToolBar("dahook");
        toolbar->setMovable(false);
        modeLabel = new QLabel("terminal", this);
        toolbar->addWidget(modeLabel);
        urlBar = new QLineEdit(this);
        urlBar->setPlaceholderText("URL (modo browser)");
        urlBar->setClearButtonEnabled(true);
        urlBar->setMinimumWidth(400);
        urlAction = toolbar->addWidget(urlBar);
        QWidget *spacer = new QWidget(this);
        spacer->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
        toolbar->addWidget(spacer);
        xButton = new QPushButton("✕", this);
        xButton->setToolTip("voltar ao terminal");
        xButton->setFixedWidth(48);
        toolbar->addWidget(xButton);

        stack = new QStackedWidget(this);
        setCentralWidget(stack);
        stack->addWidget(buildTerminal());
        stack->addWidget(buildBrowser());

        connect(xButton, &QPushButton::clicked, this, &DahookWindow::onX);
        connect(urlBar, &QLineEdit::returnPressed, this, &DahookWindow::onUrlEntered);
        connect(view, &QWebEngineView::urlChanged, this, [this](const QUrl &u) {
            if (urlBar->text() != u.toString()) urlBar->setText(u.toString());
        });

        buildShortcuts();
        toTerminal();
        startShell();

        QTimer *poll = new QTimer(this);
        connect(poll, &QTimer::timeout, this, &DahookWindow::checkTrigger);
        poll->start(250);
    }

private:
    QToolBar *toolbar = nullptr;
    QLabel *modeLabel = nullptr;
    QLineEdit *urlBar = nullptr;
    QAction *urlAction = nullptr;
    QPushButton *xButton = nullptr;
    QStackedWidget *stack = nullptr;
    TermView *termLog = nullptr;
    PtyShell *shell = nullptr;
    LineBuf lineBuf;
    QElapsedTimer readyTimer;
    bool readyOk = false;
    QWebEngineView *view = nullptr;
    QList<QShortcut *> termKeys, webKeys;

    QWidget *buildTerminal() {
        QWidget *w = new QWidget(this);
        QVBoxLayout *lay = new QVBoxLayout(w);
        termLog = new TermView(w);
        termLog->setMaximumBlockCount(5000);
        QFont f("monospace");
        termLog->setFont(f);
        lay->addWidget(termLog);
        return w;
    }

    QWidget *buildBrowser() {
        QWidget *w = new QWidget(this);
        QVBoxLayout *lay = new QVBoxLayout(w);
        lay->setContentsMargins(0, 0, 0, 0);
        QWebEngineProfile *prof = new QWebEngineProfile("dahook", this);
        prof->setPersistentStoragePath(kProfile());
        prof->setCachePath(kProfile() + "/cache");
        view = new QWebEngineView(w);
        view->setPage(new QWebEnginePage(prof, view));
        view->setUrl(QUrl(kHomeUrl));
        lay->addWidget(view);
        return w;
    }

    void startShell() {
        writeFile(kRcFile(), bashRcContent());
        QFile::remove(kReady());
        readyOk = false;
        readyTimer.start();
        if (!shell) {
            shell = new PtyShell(this);
            connect(shell, &PtyShell::output, this, &DahookWindow::onShellOutput);
            connect(shell, &PtyShell::exited, this, &DahookWindow::onShellExited);
            connect(termLog, &TermView::sendKeys, this, [this](const QByteArray &b) {
                shell->writeRaw(b);
            });
            QColor bgc = termLog->palette().color(QPalette::Base); // cor real p/ OSC 11
            auto hex2 = [](int v) { return QString("%1").arg(v, 2, 16, QChar('0')); };
            shell->setBgRgb(hex2(bgc.red()) + hex2(bgc.red()) + "/" + hex2(bgc.green()) + hex2(bgc.green()) +
                             "/" + hex2(bgc.blue()) + hex2(bgc.blue()));
        }
        if (!shell->start()) {
            note("[dahook-ERRO] pty/bash não iniciou.");
            return;
        }
        // Terminal nasce limpo: sem banner. O rcfile sinaliza o ready
        // sozinho; o poll abaixo só fala se ALGO DER ERRADO (sem eco no shell).
    }

    void buildShortcuts() {
        auto add = [&](const char *seq, QList<QShortcut *> &bucket, auto slot) {
            QShortcut *s = new QShortcut(QKeySequence(seq), this);
            s->setContext(Qt::WindowShortcut);
            connect(s, &QShortcut::activated, this, slot);
            bucket << s;
        };
        add("Ctrl+Shift+C", termKeys, [this]() { termLog->copy(); });
        add("Ctrl+Shift+V", termKeys, [this]() {
            shell->writeRaw(QGuiApplication::clipboard()->text().toLocal8Bit());
        });
        add("Ctrl+Shift+Plus", termKeys, [this]() { termLog->zoomIn(1); });
        add("Ctrl+Shift+Minus", termKeys, [this]() { termLog->zoomOut(1); });
        // Ctrl+C / Ctrl+L / setas / Tab vão direto pro shell (termKeyBytes);
        // Ctrl+Shift+C/V ficam no app (copiar/colar).
        add("Ctrl+L", webKeys, [this]() { urlBar->setFocus(); urlBar->selectAll(); });
        add("F5", webKeys, [this]() { view->reload(); });
        add("Ctrl+R", webKeys, [this]() { view->reload(); });
        add("Ctrl+Shift+R", webKeys, [this]() { view->page()->triggerAction(QWebEnginePage::ReloadAndBypassCache); });
        add("Alt+Left", webKeys, [this]() { view->back(); });
        add("Alt+Right", webKeys, [this]() { view->forward(); });
        add("Escape", webKeys, [this]() { view->stop(); });
        add("Ctrl+F", webKeys, [this]() {
            bool ok = false;
            QString q = QInputDialog::getText(this, "buscar", "buscar na página:", QLineEdit::Normal, {}, &ok);
            if (ok && !q.isEmpty()) view->findText(q);
        });
        add("Ctrl+Plus", webKeys, [this]() { view->setZoomFactor(view->zoomFactor() + 0.1); });
        add("Ctrl+Minus", webKeys, [this]() { view->setZoomFactor(view->zoomFactor() - 0.1); });
        add("Ctrl+0", webKeys, [this]() { view->setZoomFactor(1.0); });
    }

    void setKeys(bool terminalMode) {
        for (auto *s : termKeys) s->setEnabled(terminalMode);
        for (auto *s : webKeys) s->setEnabled(!terminalMode);
    }

    void toTerminal() {
        stack->setCurrentIndex(0);
        modeLabel->setText("terminal");
        urlAction->setVisible(false);
        xButton->setToolTip("fechar dahook");
        setKeys(true);
        writeFile(kMode(), "terminal");
        termLog->setFocus();
    }

    void toBrowser(const QString &url) {
        QString u = url.trimmed().isEmpty() ? QString(kHomeUrl) : url.trimmed();
        stack->setCurrentIndex(1);
        modeLabel->setText("browser");
        urlAction->setVisible(true);
        urlBar->setText(u);
        xButton->setToolTip("voltar ao terminal");
        setKeys(false);
        writeFile(kMode(), "browser");
        view->setUrl(QUrl::fromUserInput(u));
        view->setFocus();
    }

private slots:
    // Renderiza o LineBuf: o documento termina sempre num bloco "vivo" (pending).
    void renderOut() {
        QTextCursor cur(termLog->document());
        cur.movePosition(QTextCursor::End);
        cur.movePosition(QTextCursor::StartOfBlock, QTextCursor::KeepAnchor);
        cur.removeSelectedText();
        for (const QString &ln : lineBuf.done) cur.insertText(ln + "\n");
        lineBuf.done.clear();
        cur.insertText(lineBuf.pending);
        termLog->verticalScrollBar()->setValue(termLog->verticalScrollBar()->maximum());
    }
    void note(const QString &msg) {
        feedLines(lineBuf, msg + "\n");
        renderOut();
    }
    void onShellOutput(QByteArray data) {
        AnsiResult r = filterAnsi(data);
        if (r.clear) {
            termLog->clear();
            lineBuf = LineBuf();
        }
        QString t = QString::fromUtf8(r.bytes);
        if (t.isEmpty()) return;
        feedLines(lineBuf, t);
        renderOut();
    }
    void onShellExited() {
        if (shell->uptimeMs() >= 0 && shell->uptimeMs() < 2000) {
            note("[dahook-ERRO] bash morreu ao nascer; sem respawn.");
            return;
        }
        note("[bash saiu — reiniciando shell em ~]");
        startShell();
    }
    void onUrlEntered() {
        if (!urlBar->text().trimmed().isEmpty()) view->setUrl(QUrl::fromUserInput(urlBar->text()));
    }
    void onX() {
        if (stack->currentIndex() == 1) toTerminal();
        else close();
    }
    void checkTrigger() {
        QString url = readTrigger();
        if (!url.isNull()) {
            toBrowser(url);
            return;
        }
        if (!readyOk) {
            if (QFile::exists(kReady())) {
                QFile::remove(kReady());
                readyOk = true;
            } else if (readyTimer.isValid() && readyTimer.elapsed() > 5000) {
                readyOk = true;
                note("[dahook] ERRO: shell iniciou sem a função dahook.");
            }
        }
    }
};

#include "main.moc"

int main(int argc, char **argv) {
    for (int i = 1; i < argc; i++) {
        if (QString(argv[i]) == "--selftest") return selftest();
    }
    QApplication app(argc, argv);
    DahookWindow w;
    w.show();
    return app.exec();
}
