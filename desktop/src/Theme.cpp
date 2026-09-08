#include "Theme.h"

#include <QFile>
#include <QFontDatabase>
#include <QTextStream>

#include <cmath>

namespace nylon {

namespace {

int hexDigit(QChar c)
{
    const ushort u = c.unicode();
    if (u >= '0' && u <= '9') {
        return u - '0';
    }
    if (u >= 'a' && u <= 'f') {
        return 10 + (u - 'a');
    }
    if (u >= 'A' && u <= 'F') {
        return 10 + (u - 'A');
    }
    return -1;
}

// Reads exactly two hex digits. Rejects signs, whitespace, and anything
// else QString::toInt would tolerate.
bool hexByte(const QString& s, qsizetype at, int* out)
{
    const int hi = hexDigit(s.at(at));
    const int lo = hexDigit(s.at(at + 1));
    if (hi < 0 || lo < 0) {
        return false;
    }
    *out = hi * 16 + lo;
    return true;
}

// Largest metric accepted, in pixels or counts. Keeps layout arithmetic
// far from int overflow even after multiplication by track counts.
constexpr double kMaxMetric = 100000.0;

bool validMetric(double v)
{
    return std::isfinite(v) && v >= 0.0 && v <= kMaxMetric;
}

// Bounds applied to metrics that reach the Qt style sheet.
constexpr int kMaxStyleBorder = 16;
constexpr int kMaxStylePadding = 64;
constexpr int kMinStyleControlHeight = 8;
constexpr int kMaxStyleControlHeight = 256;

} // namespace

bool Theme::parseColor(const QString& text, QColor* out)
{
    // Accept #RRGGBB and #RRGGBBAA. QColor's own parser reads eight digits as
    // AARRGGBB, which is the opposite of the theme convention, so decode here.
    if (!text.startsWith(QLatin1Char('#'))) {
        return false;
    }
    const qsizetype len = text.size();
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
    for (qsizetype i = 0; i < lines.size(); ++i) {
        const int lineNo = static_cast<int>(i) + 1;
        const QString line = lines.at(i).trimmed();
        if (line.isEmpty() || line.startsWith(QLatin1Char('#'))) {
            continue;
        }
        const qsizetype eq = line.indexOf(QLatin1Char('='));
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
            } else if (!validMetric(number)) {
                report(lineNo, QStringLiteral("metric '%1' must be finite and within 0 to %2")
                    .arg(key).arg(kMaxMetric, 0, 'f', 0));
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
        for (const char* extra : {"state.on", "state.solo", "state.arm", "state.play", "state.record",
                 "state.loop", "browser.background", "browser.selection", "browser.header",
                 "detail.background", "mixer.background", "fader.track", "fader.fill", "fader.handle",
                 "knob.track", "knob.arc", "meter.background", "clip.empty.hover", "track.text", "lcd.background",
                 "lcd.text", "lcd.dim", "window.border",
                 "titlebar.background", "panel.border", "window.control.close", "window.control.minimize",
                 "window.control.zoom"}) {
            k.append(QString::fromLatin1(extra));
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
        QStringLiteral("text.inset"),
        QStringLiteral("transport.spacing"),
        QStringLiteral("transport.tempo.width"),
        QStringLiteral("session.header.band"),
        QStringLiteral("session.stop.size"),
        QStringLiteral("session.stop.inset"),
        QStringLiteral("arrangement.header.band"),
        QStringLiteral("browser.width"),
        QStringLiteral("detail.height"),
        QStringLiteral("detail.width"),
        QStringLiteral("mixer.height"),
        QStringLiteral("fader.width"),
        QStringLiteral("fader.handle.height"),
        QStringLiteral("knob.size"),
        QStringLiteral("meter.channel.width"),
        QStringLiteral("meter.clip.height"),
        QStringLiteral("strip.button.height"),
        QStringLiteral("transport.button.size"),
        QStringLiteral("radius"),
        QStringLiteral("radius.small"),
        QStringLiteral("panel.gap"),
        QStringLiteral("panel.padding"),
        QStringLiteral("titlebar.height"),
        QStringLiteral("window.border"),
        QStringLiteral("window.control.size"),
        QStringLiteral("lcd.width"),
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

QString Theme::resolvedFontFamily() const
{
    const QString requested = m_fonts.value(QStringLiteral("family"), QStringLiteral("system"));
    QString family = requested == QLatin1String("system")
        ? QFontDatabase::systemFont(QFontDatabase::GeneralFont).family()
        : requested;
    // Some platform backends advertise generic families that hasFamily does
    // not recognize. Preserve an explicit selection from that advertised list.
    const bool listedSelection = requested != QLatin1String("system")
        && QFontDatabase::families().contains(family, Qt::CaseInsensitive);
    if (QFontDatabase::hasFamily(family) || listedSelection) {
        return family;
    }
    // The family is not installed (a theme naming an absent font, or a
    // platform plugin whose default is a placeholder name). Pick an installed
    // family directly rather than letting the font matcher hunt for aliases.
    static const char* const preferred[] = {
        "Helvetica Neue", "Helvetica", "Segoe UI", "Noto Sans", "DejaVu Sans",
        "Liberation Sans", "Arial", "Cantarell", "Ubuntu",
    };
    for (const char* candidate : preferred) {
        const QString name = QString::fromLatin1(candidate);
        if (QFontDatabase::hasFamily(name)) {
            return name;
        }
    }
    const QStringList all = QFontDatabase::families();
    return all.isEmpty() ? family : all.first();
}

QFont Theme::resolvedFont() const
{
    QFont font(resolvedFontFamily());
    font.setPixelSize(qMax(1, metricInt(QStringLiteral("font.size"), 11)));
    return font;
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
    // Widget borders, padding, and heights are clamped to ranges a style
    // sheet can render; the views use the unclamped values with 64-bit
    // arithmetic instead.
    const int sep = qBound(0, metricInt(QStringLiteral("separator"), 1), kMaxStyleBorder);
    const int pad = qBound(0, metricInt(QStringLiteral("control.padding"), 4), kMaxStylePadding);
    const int ctrlH = qBound(kMinStyleControlHeight, metricInt(QStringLiteral("control.height"), 20), kMaxStyleControlHeight);
    const int radius = qBound(0, metricInt(QStringLiteral("radius"), 8), 32);
    const int radiusSmall = qBound(0, metricInt(QStringLiteral("radius.small"), 4), 16);
    const int menuPad = qMax(2, pad - 2);
    const int menuWidth = ctrlH * 8;
    return QStringLiteral(
        "QMainWindow, QDialog, QWidget#central { background: %BG; }"
        "QWidget { color: %TEXT; background: %BG; }"
        "QWidget#titleBar { background: %TITLEBG; }"
        "QLabel#windowTitle { color: %SECONDARY; font-weight: 600; }"
        "QMenuBar { background: transparent; color: %TEXT; border: none; padding: 0; }"
        "QMenuBar::item { padding: %MENUPADpx %PAD2px; background: transparent; border-radius: %RSMALLpx; }"
        "QMenuBar::item:selected { background: %HOVER; }"
        "QMenuBar::item:pressed { background: %PRESSED; }"
        "QMenu { background: %RAISED; color: %TEXT; border: %SEPpx solid %BORDER; border-radius: %RADIUSpx; padding: %MENUPADpx; min-width: %MENUWpx; }"
        "QMenu::item { padding: %MENUPADpx %PAD2px; border-radius: %RSMALLpx; }"
        "QMenu::item:selected { background: %ACCENT; color: %ACCENTTEXT; }"
        "QMenu::item:disabled { color: %DISABLED; }"
        "QMenu::separator { height: %SEPpx; background: %SEPC; margin: %MENUPADpx 0; }"
        "QPushButton, QToolButton { background: %CTRLBG;"
        "  color: %CTRLTEXT; border: %SEPpx solid %BORDER;"
        "  border-radius: %RSMALLpx; padding: 0 %PAD2px; min-height: %CTRLHpx; max-height: %CTRLHpx; }"
        "QPushButton:hover, QToolButton:hover { background: %HOVER; }"
        "QPushButton:pressed, QToolButton:pressed, QToolButton:checked { background: %PRESSED; }"
        "QToolButton:checked { color: %ACCENT; border-bottom: %SEP2px solid %ACCENT; }"
        "QPushButton:disabled, QToolButton:disabled { color: %DISABLED; }"
        "QDoubleSpinBox, QSpinBox, QLineEdit { background: %CTRLBG; color: %CTRLTEXT; border: %SEPpx solid %BORDER;"
        "  border-radius: %RSMALLpx; padding: 0 %PADpx; min-height: %CTRLHpx; max-height: %CTRLHpx;"
        "  selection-background-color: %ACCENT; selection-color: %ACCENTTEXT; }"
        "QDoubleSpinBox::up-button, QDoubleSpinBox::down-button { width: 0; border: none; }"
        "QLabel { background: transparent; }"
        "QLabel#secondary { color: %SECONDARY; }"
        "QStatusBar { background: transparent; color: %SECONDARY; border: none; }"
        "QStatusBar::item { border: none; }"
        "QSplitter::handle { background: transparent; }"
        "QSplitter::handle:horizontal { width: %SEPpx; }"
        "QSplitter::handle:vertical { height: %SEPpx; }"
        "QScrollBar:vertical { background: transparent; width: 10px; margin: 0; border: none; }"
        "QScrollBar:horizontal { background: transparent; height: 10px; margin: 0; border: none; }"
        "QScrollBar::handle { background: %RAISED; border: 2px solid transparent; border-radius: 5px; min-height: 20px; min-width: 20px; }"
        "QScrollBar::handle:hover { background: %HOVER; }"
        "QScrollBar::add-line, QScrollBar::sub-line { width: 0; height: 0; border: none; background: none; }"
        "QScrollBar::add-page, QScrollBar::sub-page { background: none; }"
        "QToolTip { background: %RAISED; color: %TEXT; border: %SEPpx solid %BORDER; border-radius: %RSMALLpx; padding: %PADpx; }"
        "QListWidget, QTreeView, QListView { background: transparent; color: %TEXT; border: none; outline: none;"
        "  selection-background-color: %BROWSERSEL; selection-color: %TEXT; show-decoration-selected: 1; }"
        "QListWidget::item, QTreeView::item { padding: %PADpx %PAD2px; border: none; border-radius: %RSMALLpx; }"
        "QListWidget::item:selected, QTreeView::item:selected { background: %BROWSERSEL; color: %TEXT; }"
        "QListWidget::item:hover, QTreeView::item:hover { background: %HOVER; }"
        "QTreeView::branch { background: transparent; }"
        "QComboBox { background: %CTRLBG; color: %CTRLTEXT; border: %SEPpx solid %BORDER; border-radius: %RSMALLpx;"
        "  padding: 0 %PADpx; min-height: %CTRLHpx; }"
        "QComboBox::drop-down { border: none; width: 16px; }"
        "QComboBox QAbstractItemView { background: %RAISED; color: %TEXT; selection-background-color: %ACCENT;"
        "  selection-color: %ACCENTTEXT; border: %SEPpx solid %BORDER; }"
        "QDialog { background: %BG; }"
        "QWidget#startCard { background: transparent; }"
        "QWidget#startSidebar, QWidget#startRecent { background: %PANEL; border: %SEPpx solid %BORDER; border-radius: %RADIUSpx; }"
        "QLabel#startTitle { color: %TEXT; }"
        "QLabel#panelTitle { color: %TEXT; font-weight: 600; padding: %PADpx %PAD2px; }"
        "QWidget#viewBar, QWidget#lowerDockHeader { background: %PANEL; border-top: %SEPpx solid %SEPC; }"
        "QWidget#lowerDock { background: %PANEL; }"
        "QLabel#positionDisplay { background: %CTRLBG; border: %SEPpx solid %BORDER; border-radius: %RSMALLpx; padding: 0 %PADpx; }"
        "QLabel#positionDisplay:disabled { color: %DISABLED; }"
        "QLabel#detailTitle { color: %TEXT; padding-left: %PADpx; }"
        "QLabel#stripName { color: palette(window-text); }"
        "QSplitter::handle { background: %SEPC; }")
        .replace(QLatin1String("%MENUW"), QString::number(menuWidth))
        .replace(QLatin1String("%MENUPAD"), QString::number(menuPad))
        .replace(QLatin1String("%TITLEBG"), c("titlebar.background"))
        .replace(QLatin1String("%BROWSERBG"), c("browser.background"))
        .replace(QLatin1String("%BROWSERSEL"), c("browser.selection"))
        .replace(QLatin1String("%BG"), c("background"))
        .replace(QLatin1String("%PANEL"), c("panel"))
        .replace(QLatin1String("%RAISED"), c("raised"))
        .replace(QLatin1String("%SEPC"), c("separator"))
        .replace(QLatin1String("%RADIUS"), QString::number(radius))
        .replace(QLatin1String("%RSMALL"), QString::number(radiusSmall))
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
