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
    p.setPen(QPen(theme.color(QStringLiteral("panel.border")), 1));
    p.setBrush(Qt::NoBrush);
    p.drawPath(path);
    p.restore();
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
