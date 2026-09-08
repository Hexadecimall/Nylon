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
    // A lit top edge. Panels read as surfaces rather than as flat fields,
    // and a theme that wants no relief sets the metric to zero.
    const int depth = qBound(0, theme.metricInt(QStringLiteral("depth"), 0), 60);
    if (depth > 0 && rect.height() > 2 * radius + 2) {
        QLinearGradient sheen(QPointF(rect.left(), rect.top()), QPointF(rect.left(), rect.top() + radius + 2));
        QColor top = fill.lighter(100 + depth);
        top.setAlpha(255);
        sheen.setColorAt(0.0, top);
        QColor fade = top;
        fade.setAlpha(0);
        sheen.setColorAt(1.0, fade);
        p.save();
        p.setClipPath(path);
        p.fillRect(QRect(rect.left(), rect.top(), rect.width(), radius + 2), sheen);
        p.restore();
    }
    p.setPen(QPen(theme.color(QStringLiteral("panel.border")), 1));
    p.setBrush(Qt::NoBrush);
    p.drawPath(path);
    p.restore();
}

// Fills a control shape with a flat surface and one-pixel outline.
inline void control(QPainter& p, const Theme& theme, const QPainterPath& shape, const QColor& base, bool sunken = false)
{
    p.save();
    p.setRenderHint(QPainter::Antialiasing, true);
    p.fillPath(shape, sunken ? base.darker(108) : base);
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
