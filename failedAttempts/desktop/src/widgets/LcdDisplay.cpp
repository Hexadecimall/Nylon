#include "LcdDisplay.h"

#include "PanelPaint.h"
#include "Theme.h"

#include <QFontDatabase>
#include <QPainter>

namespace nylon {

LcdDisplay::LcdDisplay(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
{
    setObjectName(QStringLiteral("lcd"));
}

void LcdDisplay::setTheme(const Theme* theme)
{
    m_theme = theme;
    updateGeometry();
    update();
}

void LcdDisplay::setPosition(const QString& barsBeats)
{
    m_position = barsBeats;
    update();
}

void LcdDisplay::setTempo(double bpm)
{
    m_tempo = bpm;
    update();
}

void LcdDisplay::setSignature(int numerator, int denominator)
{
    m_numerator = numerator;
    m_denominator = denominator;
    update();
}

void LcdDisplay::setKey(const QString& key)
{
    m_key = key;
    update();
}

QString LcdDisplay::tempoText() const
{
    return QString::number(m_tempo, 'f', 2);
}

QSize LcdDisplay::sizeHint() const
{
    return QSize(m_theme->metricInt(QStringLiteral("lcd.width"), 300), m_theme->metricInt(QStringLiteral("transport.height"), 28) + 4);
}

void LcdDisplay::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const QPainterPath shape = paint::rounded(*m_theme, QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), false);
    paint::control(p, *m_theme, shape, m_theme->color(QStringLiteral("lcd.background")), true);

    QFont mono = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    mono.setPixelSize(m_theme->metricInt(QStringLiteral("font.size"), 11) + 3);
    QFont label = font();
    label.setPixelSize(qMax(7, m_theme->metricInt(QStringLiteral("font.size"), 11) - 3));
    const QColor bright = isEnabled() ? m_theme->color(QStringLiteral("lcd.text")) : m_theme->color(QStringLiteral("lcd.dim"));
    const QColor dim = m_theme->color(QStringLiteral("lcd.dim"));

    // Four fields: position | tempo | signature | key.
    struct Field {
        QString label;
        QString value;
        int weight;
    };
    const Field fields[] = {
        {tr("POSITION"), m_position, 5},
        {tr("TEMPO"), tempoText(), 3},
        {tr("SIG"), QStringLiteral("%1/%2").arg(m_numerator).arg(m_denominator), 2},
        {tr("KEY"), m_key.isEmpty() ? QStringLiteral("-") : m_key, 2},
    };
    int totalWeight = 0;
    for (const Field& f : fields) {
        totalWeight += f.weight;
    }
    const int pad = 8;
    int x = pad;
    const int inner = width() - 2 * pad;
    for (const Field& f : fields) {
        const int w = inner * f.weight / totalWeight;
        const QRect cell(x, 0, w, height());
        p.setFont(label);
        p.setPen(dim);
        p.drawText(cell.adjusted(0, 2, 0, 0), Qt::AlignHCenter | Qt::AlignTop, f.label);
        p.setFont(mono);
        p.setPen(bright);
        p.drawText(cell.adjusted(0, 6, 0, -1), Qt::AlignHCenter | Qt::AlignVCenter, f.value);
        if (&f != &fields[3]) {
            p.fillRect(QRect(x + w - 1, 6, 1, height() - 12), dim.darker(150));
        }
        x += w;
    }
}

} // namespace nylon
