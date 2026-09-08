#include "Knob.h"

#include "Theme.h"

#include <QPainter>
#include <QPainterPath>
#include <QtMath>

namespace nylon {

Knob::Knob(const Theme* theme, QWidget* parent)
    : ControlWidget(theme, parent)
{
    setDragPixels(150);
}

void Knob::setBipolar(bool bipolar)
{
    m_bipolar = bipolar;
    update();
}

void Knob::setDisplayText(const QString& text)
{
    m_displayText = text;
    update();
}

QString Knob::displayText() const
{
    if (!m_displayText.isEmpty()) {
        return m_displayText;
    }
    return QStringLiteral("%1%2").arg(value(), 0, 'f', 1).arg(unit());
}

QSize Knob::sizeHint() const
{
    const int d = theme()->metricInt(QStringLiteral("knob.size"), 22);
    return QSize(d + 8, d + theme()->metricInt(QStringLiteral("font.size"), 11) + 6);
}

void Knob::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing, true);
    const Theme* t = theme();
    const int d = t->metricInt(QStringLiteral("knob.size"), 22);
    const QRectF ring((width() - d) / 2.0, 1.0, d, d);
    const double thickness = qMax(2.0, d / 9.0);
    const QRectF arcRect = ring.adjusted(thickness / 2, thickness / 2, -thickness / 2, -thickness / 2);

    // Sweep runs 270 degrees from 225 (lower left) clockwise to -45.
    const int startAngle = 225 * 16;
    const int span = -270 * 16;
    QPen track(t->color(QStringLiteral("knob.track")), thickness, Qt::SolidLine, Qt::FlatCap);
    p.setPen(track);
    p.drawArc(arcRect, startAngle, span);

    const QColor arcColor = isEnabled() ? t->color(QStringLiteral("knob.arc"))
                                        : t->color(QStringLiteral("text.disabled"));
    QPen arc(arcColor, thickness, Qt::SolidLine, Qt::FlatCap);
    p.setPen(arc);
    const double n = normalized();
    if (m_bipolar) {
        const int center = startAngle + span / 2;
        const int delta = static_cast<int>(qRound((n - 0.5) * span));
        p.drawArc(arcRect, center, delta);
    } else {
        p.drawArc(arcRect, startAngle, static_cast<int>(qRound(n * span)));
    }

    // Pointer.
    const double angle = qDegreesToRadians(225.0 - 270.0 * n);
    const QPointF c = ring.center();
    const double r = d / 2.0 - thickness;
    p.setPen(QPen(isEnabled() ? t->color(QStringLiteral("text.primary"))
                              : t->color(QStringLiteral("text.disabled")), qMax(1.0, thickness * 0.6)));
    p.drawLine(c + QPointF(qCos(angle) * r * 0.35, -qSin(angle) * r * 0.35),
        c + QPointF(qCos(angle) * r, -qSin(angle) * r));

    p.setPen(isEnabled() ? t->color(QStringLiteral("text.secondary"))
                         : t->color(QStringLiteral("text.disabled")));
    p.drawText(QRect(0, static_cast<int>(ring.bottom()) + 1, width(), height() - static_cast<int>(ring.bottom()) - 1),
        Qt::AlignHCenter | Qt::AlignTop, displayText());
}

} // namespace nylon
