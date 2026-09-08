#pragma once

#include "Theme.h"

#include <QPainter>
#include <QPainterPath>
#include <QRect>

namespace nylon::paint {

// Fills `rect` as a rounded panel with the theme's panel border.
inline void panel(QPainter& p, const Theme& theme, const QRect& rect, const QColor& fill)
{
    const int radius = qBound(0, theme.metricInt(QStringLiteral("radius"), 8), 32);
    p.save();
    p.setRenderHint(QPainter::Antialiasing, true);
    QPainterPath path;
    path.addRoundedRect(QRectF(rect).adjusted(0.5, 0.5, -0.5, -0.5), radius, radius);
    p.fillPath(path, fill);
    // Top-edge highlight gives the panel a lit upper edge.
    p.save();
    p.setClipPath(path);
    p.fillRect(QRectF(rect.left(), rect.top(), rect.width(), 1.0), QColor(255, 255, 255, 14));
    p.restore();
    p.setPen(QPen(theme.color(QStringLiteral("panel.border")), 1));
    p.setBrush(Qt::NoBrush);
    p.drawPath(path);
    p.restore();
}

// Fills a control shape with a vertical gradient (lighter at the top) and a
// one-pixel highlight along the top edge, the depth cue used by every
// button, field, and cap.
inline void control(QPainter& p, const Theme& theme, const QPainterPath& shape, const QColor& base, bool sunken = false)
{
    QLinearGradient g(shape.boundingRect().topLeft(), shape.boundingRect().bottomLeft());
    if (sunken) {
        g.setColorAt(0.0, base.darker(120));
        g.setColorAt(1.0, base);
    } else {
        g.setColorAt(0.0, base.lighter(116));
        g.setColorAt(1.0, base.darker(104));
    }
    p.save();
    p.setRenderHint(QPainter::Antialiasing, true);
    p.fillPath(shape, g);
    p.setClipPath(shape);
    QColor edge = sunken ? QColor(0, 0, 0, 70) : QColor(255, 255, 255, 28);
    const QRectF r = shape.boundingRect();
    p.fillRect(QRectF(r.left(), r.top(), r.width(), 1.0), edge);
    p.restore();
    p.setPen(QPen(theme.color(QStringLiteral("control.border")), 1));
    p.setBrush(Qt::NoBrush);
    p.drawPath(shape);
}

// Rounded rectangle path with the small control radius.
inline QPainterPath rounded(const Theme& theme, const QRectF& rect, bool small = true)
{
    const int r = small ? qBound(0, theme.metricInt(QStringLiteral("radius.small"), 4), 16)
                        : qBound(0, theme.metricInt(QStringLiteral("radius"), 8), 32);
    QPainterPath path;
    path.addRoundedRect(rect, r, r);
    return path;
}

// Clip path matching a panel outline so children stay inside the corners.
inline QPainterPath clip(const Theme& theme, const QRect& rect)
{
    return rounded(theme, QRectF(rect), false);
}

} // namespace nylon::paint
