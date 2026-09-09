#include "Icons.h"

#include <QPainter>
#include <QPainterPath>
#include <QPixmap>
#include <QPolygonF>

#include <cmath>

namespace nylon {

namespace {

// Every path below is drawn inside a unit square and then mapped onto the
// box, which keeps the shapes consistent between sizes.
QPointF at(const QRectF& box, double x, double y)
{
    return QPointF(box.left() + x * box.width(), box.top() + y * box.height());
}

QRectF unitRect(const QRectF& box, double x, double y, double w, double h)
{
    return QRectF(at(box, x, y), QSizeF(w * box.width(), h * box.height()));
}

void strokeLine(QPainter& p, const QRectF& box, double x1, double y1, double x2, double y2)
{
    p.drawLine(at(box, x1, y1), at(box, x2, y2));
}

void paintWaveform(QPainter& p, const QRectF& box)
{
    static const double heights[] = {0.45, 0.95, 0.7, 1.0, 0.55};
    const int bars = 5;
    for (int index = 0; index < bars; ++index) {
        const double x = 0.1 + index * 0.2;
        const double half = heights[index] * 0.4;
        strokeLine(p, box, x, 0.5 - half, x, 0.5 + half);
    }
}

void paintNotes(QPainter& p, const QRectF& box)
{
    p.drawEllipse(unitRect(box, 0.05, 0.62, 0.3, 0.28));
    p.drawEllipse(unitRect(box, 0.6, 0.5, 0.3, 0.28));
    strokeLine(p, box, 0.33, 0.76, 0.33, 0.16);
    strokeLine(p, box, 0.88, 0.64, 0.88, 0.06);
    strokeLine(p, box, 0.33, 0.16, 0.88, 0.06);
}

void paintDrum(QPainter& p, const QRectF& box)
{
    p.drawEllipse(unitRect(box, 0.08, 0.12, 0.84, 0.34));
    strokeLine(p, box, 0.08, 0.29, 0.08, 0.71);
    strokeLine(p, box, 0.92, 0.29, 0.92, 0.71);
    QPainterPath bottom;
    bottom.moveTo(at(box, 0.08, 0.71));
    bottom.cubicTo(at(box, 0.25, 0.95), at(box, 0.75, 0.95), at(box, 0.92, 0.71));
    p.drawPath(bottom);
}

void paintInstrument(QPainter& p, const QRectF& box)
{
    // A keyboard: one long key row with three black keys over it.
    p.drawRect(unitRect(box, 0.06, 0.24, 0.88, 0.52));
    strokeLine(p, box, 0.35, 0.24, 0.35, 0.76);
    strokeLine(p, box, 0.64, 0.24, 0.64, 0.76);
    const QColor pen = p.pen().color();
    p.fillRect(unitRect(box, 0.26, 0.24, 0.1, 0.3), pen);
    p.fillRect(unitRect(box, 0.55, 0.24, 0.1, 0.3), pen);
}

void paintEffect(QPainter& p, const QRectF& box)
{
    // A response curve, which is what an audio effect draws.
    QPainterPath curve;
    curve.moveTo(at(box, 0.06, 0.72));
    curve.cubicTo(at(box, 0.34, 0.72), at(box, 0.36, 0.16), at(box, 0.56, 0.16));
    curve.cubicTo(at(box, 0.76, 0.16), at(box, 0.76, 0.6), at(box, 0.94, 0.6));
    p.drawPath(curve);
}

void paintPlug(QPainter& p, const QRectF& box)
{
    p.drawRect(unitRect(box, 0.28, 0.34, 0.44, 0.5));
    strokeLine(p, box, 0.4, 0.34, 0.4, 0.1);
    strokeLine(p, box, 0.6, 0.34, 0.6, 0.1);
    strokeLine(p, box, 0.5, 0.84, 0.5, 0.96);
}

void paintClip(QPainter& p, const QRectF& box)
{
    // A clip: a launch triangle beside a block, the way a slot reads.
    QPolygonF play;
    play << at(box, 0.1, 0.22) << at(box, 0.1, 0.78) << at(box, 0.42, 0.5);
    p.drawPolygon(play);
    p.drawRect(unitRect(box, 0.56, 0.22, 0.36, 0.56));
}

void paintSample(QPainter& p, const QRectF& box)
{
    QPainterPath wave;
    wave.moveTo(at(box, 0.06, 0.5));
    wave.cubicTo(at(box, 0.28, 0.02), at(box, 0.38, 0.98), at(box, 0.55, 0.5));
    wave.cubicTo(at(box, 0.7, 0.16), at(box, 0.8, 0.84), at(box, 0.94, 0.5));
    p.drawPath(wave);
}

void paintGroove(QPainter& p, const QRectF& box)
{
    for (int index = 0; index < 4; ++index) {
        const double x = 0.12 + index * 0.25;
        const double height = index % 2 == 0 ? 0.34 : 0.2;
        p.drawRect(unitRect(box, x, 0.5 - height / 2.0, 0.12, height));
    }
}

void paintFolder(QPainter& p, const QRectF& box)
{
    QPainterPath folder;
    folder.moveTo(at(box, 0.08, 0.8));
    folder.lineTo(at(box, 0.08, 0.24));
    folder.lineTo(at(box, 0.42, 0.24));
    folder.lineTo(at(box, 0.5, 0.36));
    folder.lineTo(at(box, 0.92, 0.36));
    folder.lineTo(at(box, 0.92, 0.8));
    folder.closeSubpath();
    p.drawPath(folder);
}

void paintHome(QPainter& p, const QRectF& box)
{
    QPolygonF roof;
    roof << at(box, 0.08, 0.5) << at(box, 0.5, 0.14) << at(box, 0.92, 0.5);
    p.drawPolyline(roof);
    p.drawRect(unitRect(box, 0.2, 0.5, 0.6, 0.36));
}

void paintDownload(QPainter& p, const QRectF& box)
{
    strokeLine(p, box, 0.5, 0.12, 0.5, 0.64);
    QPolygonF head;
    head << at(box, 0.28, 0.44) << at(box, 0.5, 0.68) << at(box, 0.72, 0.44);
    p.drawPolyline(head);
    strokeLine(p, box, 0.14, 0.86, 0.86, 0.86);
}

void paintDesktop(QPainter& p, const QRectF& box)
{
    p.drawRect(unitRect(box, 0.08, 0.18, 0.84, 0.5));
    strokeLine(p, box, 0.32, 0.86, 0.68, 0.86);
    strokeLine(p, box, 0.5, 0.68, 0.5, 0.86);
}

void paintDocument(QPainter& p, const QRectF& box)
{
    QPainterPath page;
    page.moveTo(at(box, 0.22, 0.1));
    page.lineTo(at(box, 0.62, 0.1));
    page.lineTo(at(box, 0.8, 0.3));
    page.lineTo(at(box, 0.8, 0.9));
    page.lineTo(at(box, 0.22, 0.9));
    page.closeSubpath();
    p.drawPath(page);
    strokeLine(p, box, 0.62, 0.1, 0.62, 0.3);
    strokeLine(p, box, 0.62, 0.3, 0.8, 0.3);
}

void paintMusic(QPainter& p, const QRectF& box)
{
    p.drawEllipse(unitRect(box, 0.12, 0.6, 0.3, 0.28));
    strokeLine(p, box, 0.4, 0.74, 0.4, 0.12);
    QPainterPath flag;
    flag.moveTo(at(box, 0.4, 0.12));
    flag.cubicTo(at(box, 0.68, 0.16), at(box, 0.78, 0.3), at(box, 0.8, 0.44));
    p.drawPath(flag);
}

void paintSearch(QPainter& p, const QRectF& box)
{
    p.drawEllipse(unitRect(box, 0.12, 0.12, 0.56, 0.56));
    strokeLine(p, box, 0.66, 0.66, 0.9, 0.9);
}

void paintPlus(QPainter& p, const QRectF& box)
{
    strokeLine(p, box, 0.5, 0.16, 0.5, 0.84);
    strokeLine(p, box, 0.16, 0.5, 0.84, 0.5);
}

void paintChevron(QPainter& p, const QRectF& box)
{
    QPolygonF chevron;
    chevron << at(box, 0.36, 0.2) << at(box, 0.66, 0.5) << at(box, 0.36, 0.8);
    p.drawPolyline(chevron);
}

} // namespace

void paintIcon(QPainter& painter, Icon icon, const QRectF& box, const QColor& color)
{
    if (box.width() <= 0.0 || box.height() <= 0.0) {
        return;
    }
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, true);
    QPen pen(color);
    // A stroke a fifteenth of the box keeps small icons legible without
    // turning large ones into blobs.
    pen.setWidthF(std::max(1.0, std::min(box.width(), box.height()) / 15.0 * 2.0));
    pen.setCapStyle(Qt::RoundCap);
    pen.setJoinStyle(Qt::RoundJoin);
    painter.setPen(pen);
    painter.setBrush(Qt::NoBrush);
    switch (icon) {
    case Icon::Waveform: paintWaveform(painter, box); break;
    case Icon::Notes: paintNotes(painter, box); break;
    case Icon::Drum: paintDrum(painter, box); break;
    case Icon::Instrument: paintInstrument(painter, box); break;
    case Icon::Effect: paintEffect(painter, box); break;
    case Icon::Plug: paintPlug(painter, box); break;
    case Icon::Clip: paintClip(painter, box); break;
    case Icon::Sample: paintSample(painter, box); break;
    case Icon::Groove: paintGroove(painter, box); break;
    case Icon::Folder: paintFolder(painter, box); break;
    case Icon::Home: paintHome(painter, box); break;
    case Icon::Download: paintDownload(painter, box); break;
    case Icon::Desktop: paintDesktop(painter, box); break;
    case Icon::Document: paintDocument(painter, box); break;
    case Icon::Music: paintMusic(painter, box); break;
    case Icon::Search: paintSearch(painter, box); break;
    case Icon::Plus: paintPlus(painter, box); break;
    case Icon::Chevron: paintChevron(painter, box); break;
    }
    painter.restore();
}

QIcon iconFor(Icon icon, const QColor& color, int size, qreal devicePixelRatio)
{
    const int side = std::max(4, size);
    const qreal ratio = devicePixelRatio > 0.0 ? devicePixelRatio : 1.0;
    QPixmap pixmap(QSize(static_cast<int>(side * ratio), static_cast<int>(side * ratio)));
    pixmap.setDevicePixelRatio(ratio);
    pixmap.fill(Qt::transparent);
    QPainter painter(&pixmap);
    // The inset keeps the stroke inside the pixmap at every ratio.
    paintIcon(painter, icon, QRectF(1.0, 1.0, side - 2.0, side - 2.0), color);
    painter.end();
    return QIcon(pixmap);
}

} // namespace nylon
