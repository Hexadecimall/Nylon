#include "Theme.h"

#include <QFile>
#include <QTextStream>

namespace nylon {

namespace {

bool hexByte(const QString& s, int at, int* out)
{
    bool ok = false;
    const int v = s.mid(at, 2).toInt(&ok, 16);
    if (ok) {
        *out = v;
    }
    return ok;
}

} // namespace

bool Theme::parseColor(const QString& text, QColor* out)
{
    // Accept #RRGGBB and #RRGGBBAA. QColor's own parser reads eight digits as
    // AARRGGBB, which is the opposite of the theme convention, so decode here.
    if (!text.startsWith(QLatin1Char('#'))) {
        return false;
    }
    const int len = text.size();
    if (len != 7 && len != 9) {
        return false;
    }
    int r = 0, g = 0, b = 0, a = 255;
    if (!hexByte(text, 1, &r) || !hexByte(text, 3, &g) || !hexByte(text, 5, &b)) {
        return false;
    }
    if (len == 9 && !hexByte(text, 7, &a)) {
        return false;
    }
    *out = QColor(r, g, b, a);
    return true;
}

Theme Theme::parse(const QString& text, QStringList* errors)
{
    Theme theme;
    const auto report = [errors](int line, const QString& message) {
        if (errors) {
            errors->append(QStringLiteral("line %1: %2").arg(line).arg(message));
        }
    };

    const QStringList lines = text.split(QLatin1Char('\n'));
    for (int i = 0; i < lines.size(); ++i) {
        const int lineNo = i + 1;
        const QString line = lines.at(i).trimmed();
        if (line.isEmpty() || line.startsWith(QLatin1Char('#'))) {
            continue;
        }
        const int eq = line.indexOf(QLatin1Char('='));
        if (eq < 0) {
            report(lineNo, QStringLiteral("expected 'key = value'"));
            continue;
        }
        const QString key = line.left(eq).trimmed();
        const QString value = line.mid(eq + 1).trimmed();
        if (key.isEmpty()) {
            report(lineNo, QStringLiteral("empty key"));
            continue;
        }
        if (value.isEmpty()) {
            report(lineNo, QStringLiteral("empty value for '%1'").arg(key));
            continue;
        }

        if (key == QLatin1String("name")) {
            if (!theme.m_name.isEmpty()) {
                report(lineNo, QStringLiteral("duplicate 'name'"));
            }
            theme.m_name = value;
        } else if (key.startsWith(QLatin1String("color."))) {
            const QString sub = key.mid(6);
            QColor color;
            if (sub.isEmpty()) {
                report(lineNo, QStringLiteral("empty color key"));
            } else if (!parseColor(value, &color)) {
                report(lineNo, QStringLiteral("invalid color '%1' for '%2'").arg(value, key));
            } else {
                if (theme.m_colors.contains(sub)) {
                    report(lineNo, QStringLiteral("duplicate '%1'").arg(key));
                }
                theme.m_colors.insert(sub, color);
            }
        } else if (key.startsWith(QLatin1String("metric."))) {
            const QString sub = key.mid(7);
            bool ok = false;
            const double number = value.toDouble(&ok);
            if (sub.isEmpty()) {
                report(lineNo, QStringLiteral("empty metric key"));
            } else if (!ok) {
                report(lineNo, QStringLiteral("invalid number '%1' for '%2'").arg(value, key));
            } else {
                if (theme.m_metrics.contains(sub)) {
                    report(lineNo, QStringLiteral("duplicate '%1'").arg(key));
                }
                theme.m_metrics.insert(sub, number);
            }
        } else if (key.startsWith(QLatin1String("font."))) {
            const QString sub = key.mid(5);
            if (sub.isEmpty()) {
                report(lineNo, QStringLiteral("empty font key"));
            } else {
                if (theme.m_fonts.contains(sub)) {
                    report(lineNo, QStringLiteral("duplicate '%1'").arg(key));
                }
                theme.m_fonts.insert(sub, value);
            }
        } else {
            report(lineNo, QStringLiteral("unknown key '%1'").arg(key));
        }
    }
    return theme;
}

Theme Theme::fromFile(const QString& path, QStringList* errors)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text)) {
        if (errors) {
            errors->append(QStringLiteral("cannot read theme file"));
        }
        return Theme();
    }
    QTextStream in(&file);
    return parse(in.readAll(), errors);
}

const QStringList& Theme::requiredColors()
{
    static const QStringList keys = [] {
        QStringList k{
            QStringLiteral("background"),
            QStringLiteral("panel"),
            QStringLiteral("raised"),
            QStringLiteral("separator"),
            QStringLiteral("text.primary"),
            QStringLiteral("text.secondary"),
            QStringLiteral("text.disabled"),
            QStringLiteral("accent"),
            QStringLiteral("accent.text"),
            QStringLiteral("playhead"),
            QStringLiteral("selection"),
            QStringLiteral("session.slot"),
            QStringLiteral("session.slot.hover"),
            QStringLiteral("session.stop_button"),
            QStringLiteral("arrangement.ruler"),
            QStringLiteral("arrangement.lane"),
            QStringLiteral("arrangement.lane.alt"),
            QStringLiteral("arrangement.grid"),
            QStringLiteral("arrangement.grid.bar"),
            QStringLiteral("control.background"),
            QStringLiteral("control.border"),
            QStringLiteral("control.text"),
            QStringLiteral("control.hover"),
            QStringLiteral("control.pressed"),
            QStringLiteral("control.disabled"),
            QStringLiteral("meter.rms"),
            QStringLiteral("meter.peak"),
            QStringLiteral("meter.clip"),
        };
        for (int i = 1; i <= 16; ++i) {
            k.append(QStringLiteral("track.%1").arg(i));
        }
        return k;
    }();
    return keys;
}

const QStringList& Theme::requiredMetrics()
{
    static const QStringList keys{
        QStringLiteral("separator"),
        QStringLiteral("control.height"),
        QStringLiteral("control.padding"),
        QStringLiteral("transport.height"),
        QStringLiteral("session.slot.width"),
        QStringLiteral("session.slot.height"),
        QStringLiteral("session.scene.count"),
        QStringLiteral("session.master.width"),
        QStringLiteral("arrangement.ruler.height"),
        QStringLiteral("arrangement.lane.height"),
        QStringLiteral("arrangement.header.width"),
        QStringLiteral("arrangement.pixels_per_bar"),
        QStringLiteral("arrangement.bars"),
        QStringLiteral("font.size"),
    };
    return keys;
}

double Theme::metric(const QString& key, double fallback) const
{
    return m_metrics.value(key, fallback);
}

int Theme::metricInt(const QString& key, int fallback) const
{
    const auto it = m_metrics.constFind(key);
    return it == m_metrics.constEnd() ? fallback : qRound(*it);
}

int Theme::trackColorCount() const
{
    int n = 0;
    while (m_colors.contains(QStringLiteral("track.%1").arg(n + 1))) {
        ++n;
    }
    return n;
}

QColor Theme::trackColor(int index) const
{
    const int n = trackColorCount();
    if (n == 0 || index < 0) {
        return QColor();
    }
    return m_colors.value(QStringLiteral("track.%1").arg((index % n) + 1));
}

QStringList Theme::missingKeys() const
{
    QStringList missing;
    for (const QString& k : requiredColors()) {
        if (!m_colors.contains(k)) {
            missing.append(QStringLiteral("color.") + k);
        }
    }
    for (const QString& k : requiredMetrics()) {
        if (!m_metrics.contains(k)) {
            missing.append(QStringLiteral("metric.") + k);
        }
    }
    return missing;
}

QString Theme::styleSheet() const
{
    const auto c = [this](const char* key) { return color(QLatin1String(key)).name(QColor::HexArgb); };
    const int sep = metricInt(QStringLiteral("separator"), 1);
    const int pad = metricInt(QStringLiteral("control.padding"), 4);
    const int ctrlH = metricInt(QStringLiteral("control.height"), 20);
    const int fontPx = metricInt(QStringLiteral("font.size"), 11);
    const QString family = m_fonts.value(QStringLiteral("family"), QStringLiteral("system"));
    const QString fontFamily = family == QLatin1String("system")
        ? QString()
        : QStringLiteral("font-family: \"%1\";").arg(family);

    return QStringLiteral(
        "* { font-size: %FONTPXpx; %FAMILY }"
        "QMainWindow, QDialog, QWidget#central { background: %BG; }"
        "QWidget { color: %TEXT; background: %BG; }"
        "QMenuBar { background: %PANEL; color: %TEXT; border-bottom: %SEPpx solid %SEPC; padding: 0; }"
        "QMenuBar::item { padding: %PADpx %PAD2px; background: transparent; }"
        "QMenuBar::item:selected { background: %HOVER; }"
        "QMenu { background: %RAISED; color: %TEXT; border: %SEPpx solid %BORDER; padding: %SEPpx; }"
        "QMenu::item { padding: %PADpx %PAD3px; }"
        "QMenu::item:selected { background: %ACCENT; color: %ACCENTTEXT; }"
        "QMenu::item:disabled { color: %DISABLED; }"
        "QMenu::separator { height: %SEPpx; background: %SEPC; margin: %PADpx 0; }"
        "QPushButton, QToolButton { background: %CTRLBG; color: %CTRLTEXT; border: %SEPpx solid %BORDER;"
        "  border-radius: 0; padding: 0 %PAD2px; min-height: %CTRLHpx; max-height: %CTRLHpx; }"
        "QPushButton:hover, QToolButton:hover { background: %HOVER; }"
        "QPushButton:pressed, QToolButton:pressed, QToolButton:checked { background: %PRESSED; }"
        "QToolButton:checked { color: %ACCENT; border-bottom: %SEP2px solid %ACCENT; }"
        "QPushButton:disabled, QToolButton:disabled { color: %DISABLED; }"
        "QDoubleSpinBox, QSpinBox, QLineEdit { background: %CTRLBG; color: %CTRLTEXT; border: %SEPpx solid %BORDER;"
        "  border-radius: 0; padding: 0 %PADpx; min-height: %CTRLHpx; max-height: %CTRLHpx;"
        "  selection-background-color: %ACCENT; selection-color: %ACCENTTEXT; }"
        "QDoubleSpinBox::up-button, QDoubleSpinBox::down-button { width: 0; border: none; }"
        "QLabel { background: transparent; }"
        "QLabel#secondary { color: %SECONDARY; }"
        "QStatusBar { background: %PANEL; color: %SECONDARY; border-top: %SEPpx solid %SEPC; }"
        "QStatusBar::item { border: none; }"
        "QSplitter::handle { background: %SEPC; }"
        "QSplitter::handle:horizontal { width: %SEPpx; }"
        "QSplitter::handle:vertical { height: %SEPpx; }"
        "QScrollBar:vertical { background: %PANEL; width: 10px; margin: 0; border: none; }"
        "QScrollBar:horizontal { background: %PANEL; height: 10px; margin: 0; border: none; }"
        "QScrollBar::handle { background: %RAISED; border: %SEPpx solid %PANEL; min-height: 20px; min-width: 20px; }"
        "QScrollBar::handle:hover { background: %HOVER; }"
        "QScrollBar::add-line, QScrollBar::sub-line { width: 0; height: 0; border: none; background: none; }"
        "QScrollBar::add-page, QScrollBar::sub-page { background: none; }"
        "QToolTip { background: %RAISED; color: %TEXT; border: %SEPpx solid %BORDER; padding: %PADpx; }")
        .replace(QLatin1String("%FONTPX"), QString::number(fontPx))
        .replace(QLatin1String("%FAMILY"), fontFamily)
        .replace(QLatin1String("%BG"), c("background"))
        .replace(QLatin1String("%PANEL"), c("panel"))
        .replace(QLatin1String("%RAISED"), c("raised"))
        .replace(QLatin1String("%SEPC"), c("separator"))
        .replace(QLatin1String("%SEP2"), QString::number(sep * 2))
        .replace(QLatin1String("%SEP"), QString::number(sep))
        .replace(QLatin1String("%PAD3"), QString::number(pad * 3))
        .replace(QLatin1String("%PAD2"), QString::number(pad * 2))
        .replace(QLatin1String("%PAD"), QString::number(pad))
        .replace(QLatin1String("%CTRLH"), QString::number(ctrlH))
        .replace(QLatin1String("%TEXT"), c("text.primary"))
        .replace(QLatin1String("%SECONDARY"), c("text.secondary"))
        .replace(QLatin1String("%DISABLED"), c("text.disabled"))
        .replace(QLatin1String("%ACCENTTEXT"), c("accent.text"))
        .replace(QLatin1String("%ACCENT"), c("accent"))
        .replace(QLatin1String("%CTRLBG"), c("control.background"))
        .replace(QLatin1String("%CTRLTEXT"), c("control.text"))
        .replace(QLatin1String("%BORDER"), c("control.border"))
        .replace(QLatin1String("%HOVER"), c("control.hover"))
        .replace(QLatin1String("%PRESSED"), c("control.pressed"));
}

} // namespace nylon
