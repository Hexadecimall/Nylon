#pragma once

#include <QColor>
#include <QIcon>
#include <QRectF>

class QPainter;

namespace nylon {

// Line icons drawn from paths rather than loaded from files, so they take
// the theme's colours and stay crisp at any scale.
enum class Icon {
    Waveform,
    Notes,
    Drum,
    Instrument,
    Effect,
    Plug,
    Clip,
    Sample,
    Groove,
    Folder,
    Home,
    Download,
    Desktop,
    Document,
    Music,
    Search,
    Plus,
    Chevron,
};

// Draws one icon inside `box`, in `color`. The stroke width follows the
// box so an icon reads the same at any size.
void paintIcon(QPainter& painter, Icon icon, const QRectF& box, const QColor& color);

// The same icon as a device-pixel-aware image, for the item views that
// take one.
QIcon iconFor(Icon icon, const QColor& color, int size, qreal devicePixelRatio = 1.0);

} // namespace nylon
