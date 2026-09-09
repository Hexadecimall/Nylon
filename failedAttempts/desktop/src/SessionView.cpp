#include "SessionView.h"

#include "LayoutMath.h"
#include "PanelPaint.h"
#include "ProjectBridge.h"
#include "Theme.h"

#include <QMouseEvent>
#include <QPaintEvent>
#include <QPainter>
#include <QScrollBar>

namespace nylon {

using namespace layout;

SessionView::SessionView(ProjectBridge* bridge, const Theme* theme, QWidget* parent)
    : QAbstractScrollArea(parent)
    , m_bridge(bridge)
    , m_theme(theme)
{
    setObjectName(QStringLiteral("session"));
    setFrameShape(QFrame::NoFrame);
    viewport()->setAutoFillBackground(false);
    viewport()->setMouseTracking(true);
    connect(m_bridge, &ProjectBridge::changed, this, [this] {
        updateScrollRanges();
        if (m_selected >= columnCount()) {
            selectTrack(columnCount() - 1);
        }
        viewport()->update();
    });
    updateScrollRanges();
}

void SessionView::setTheme(const Theme* theme)
{
    m_theme = theme;
    updateScrollRanges();
    viewport()->update();
}

int SessionView::separator() const
{
    return qMax(0, m_theme->metricInt(QStringLiteral("separator"), 1));
}

int SessionView::sceneCount() const
{
    const qint64 coreScenes = static_cast<qint64>(qMin<quint64>(
        m_bridge->sceneCount(), static_cast<quint64>(layout::kMaxExtent)));
    const qint64 requested = qMax(coreScenes,
        static_cast<qint64>(qMax(1, m_theme->metricInt(QStringLiteral("session.scene.count"), 8))));
    return static_cast<int>(layoutCount(requested, slotHeight() + separator(), headerHeight() + separator()));
}

void SessionView::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton) {
        const QPoint point = event->position().toPoint();
        const int track = columnAt(point.x());
        const int scene = sceneAt(point.y());
        if (track >= 0 && scene >= 0) {
            selectTrack(track);
            emit slotCreateRequested(track, scene);
            event->accept();
            return;
        }
    }
    QAbstractScrollArea::mouseDoubleClickEvent(event);
}

int SessionView::columnCount() const
{
    return static_cast<int>(layoutCount(static_cast<qint64>(m_bridge->trackCount()), slotWidth() + separator(), 0));
}

int SessionView::slotWidth() const
{
    return qMax(24, m_theme->metricInt(QStringLiteral("session.slot.width"), 96));
}

int SessionView::slotHeight() const
{
    return qMax(10, m_theme->metricInt(QStringLiteral("session.slot.height"), 18));
}

int SessionView::headerHeight() const
{
    return m_theme->metricInt(QStringLiteral("control.height"), 20) + 4;
}

int SessionView::masterWidth() const
{
    return qMax(24, m_theme->metricInt(QStringLiteral("session.master.width"), 72));
}

bool SessionView::isShowingEmptyState() const
{
    return m_bridge->trackCount() == 0;
}

QRect SessionView::slotRect(int track, int scene) const
{
    if (track < 0 || track >= columnCount() || scene < 0 || scene >= sceneCount()) {
        return QRect();
    }
    const qint64 sep = separator();
    const qint64 x = qint64(track) * (slotWidth() + sep) - horizontalScrollBar()->value();
    const qint64 y = headerHeight() + sep + qint64(scene) * (slotHeight() + sep) - verticalScrollBar()->value();
    if (!fitsCoordinate(x) || !fitsCoordinate(y)) {
        return QRect();
    }
    return QRect(static_cast<int>(x), static_cast<int>(y), slotWidth(), slotHeight());
}

int SessionView::columnAt(int x) const
{
    const qint64 pitch = slotWidth() + separator();
    const qint64 content = qint64(x) + horizontalScrollBar()->value();
    if (content < 0) {
        return -1;
    }
    const qint64 index = content / pitch;
    if (index >= columnCount() || content - index * pitch >= slotWidth()) {
        return -1;
    }
    return static_cast<int>(index);
}

int SessionView::sceneAt(int y) const
{
    const qint64 pitch = slotHeight() + separator();
    const qint64 content = qint64(y) + verticalScrollBar()->value() - headerHeight() - separator();
    if (content < 0) {
        return -1;
    }
    const qint64 index = content / pitch;
    if (index >= sceneCount() || content - index * pitch >= slotHeight()) {
        return -1;
    }
    return static_cast<int>(index);
}

void SessionView::selectTrack(int index)
{
    if (index >= columnCount()) {
        index = columnCount() - 1;
    }
    if (index < -1) {
        index = -1;
    }
    if (m_selected == index) {
        return;
    }
    m_selected = index;
    viewport()->update();
    emit trackSelected(index);
}

void SessionView::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QAbstractScrollArea::mousePressEvent(event);
        return;
    }
    const int track = columnAt(event->pos().x());
    if (track >= 0) {
        selectTrack(track);
        const int scene = sceneAt(event->pos().y());
        if (scene >= 0) {
            emit slotClicked(track, scene);
        }
    }
    event->accept();
}

void SessionView::mouseMoveEvent(QMouseEvent* event)
{
    const int track = columnAt(event->pos().x());
    const int scene = track >= 0 ? sceneAt(event->pos().y()) : -1;
    if (track != m_hoverTrack || scene != m_hoverScene) {
        m_hoverTrack = track;
        m_hoverScene = scene;
        viewport()->update();
    }
    QAbstractScrollArea::mouseMoveEvent(event);
}

void SessionView::leaveEvent(QEvent* event)
{
    m_hoverTrack = -1;
    m_hoverScene = -1;
    viewport()->update();
    QAbstractScrollArea::leaveEvent(event);
}

void SessionView::updateScrollRanges()
{
    const qint64 sep = separator();
    const qint64 contentW = qint64(columnCount()) * (slotWidth() + sep) + masterWidth();
    const qint64 contentH = headerHeight() + sep + qint64(sceneCount()) * (slotHeight() + sep);
    horizontalScrollBar()->setRange(0, clampExtent(contentW - viewport()->width()));
    horizontalScrollBar()->setPageStep(viewport()->width());
    verticalScrollBar()->setRange(0, clampExtent(contentH - viewport()->height()));
    verticalScrollBar()->setPageStep(viewport()->height());
}

void SessionView::resizeEvent(QResizeEvent* event)
{
    QAbstractScrollArea::resizeEvent(event);
    updateScrollRanges();
}

void SessionView::paintEvent(QPaintEvent* event)
{
    QPainter p(viewport());
    paint::panel(p, *m_theme, viewport()->rect(), m_theme->color(QStringLiteral("panel")));
    p.setClipPath(paint::clip(*m_theme, viewport()->rect().adjusted(1, 1, -1, -1)));
    p.setRenderHint(QPainter::Antialiasing, true);
    p.setFont(font());
    Q_UNUSED(event);

    const int sep = separator();
    const QColor sepColor = m_theme->color(QStringLiteral("separator"));
    const QColor slotColor = m_theme->color(QStringLiteral("session.slot"));
    const QColor slotHover = m_theme->color(QStringLiteral("clip.empty.hover"));
    const QColor selection = m_theme->color(QStringLiteral("selection"));
    const QColor stopColor = m_theme->color(QStringLiteral("session.stop_button"));
    const QColor panel = m_theme->color(QStringLiteral("panel"));
    const QColor primary = m_theme->color(QStringLiteral("text.primary"));
    const QColor secondary = m_theme->color(QStringLiteral("text.secondary"));
    const int textInset = m_theme->metricInt(QStringLiteral("text.inset"), 4);
    const int band = m_theme->metricInt(QStringLiteral("session.header.band"), 2);
    const int stopSize = m_theme->metricInt(QStringLiteral("session.stop.size"), 5);
    const int stopInset = m_theme->metricInt(QStringLiteral("session.stop.inset"), 3);

    const int columns = columnCount();
    const int scenes = sceneCount();
    const int sw = slotWidth();
    const int sh = slotHeight();
    const int hh = headerHeight();
    const qint64 scrollX = horizontalScrollBar()->value();
    const qint64 scrollY = verticalScrollBar()->value();
    const int viewW = viewport()->width();
    const int viewH = viewport()->height();
    const int headerY = static_cast<int>(-scrollY);
    const qint64 gridTop = hh + sep;
    const qint64 gridHeight = qint64(scenes) * (sh + sep);

    if (m_bridge->trackCount() == 0) {
        p.setPen(secondary);
        p.drawText(viewport()->rect(), Qt::AlignCenter,
            tr("No tracks.\nAdd a track to start a session."));
    }

    qint64 firstScene = 0, lastScene = -1;
    visibleRange(scenes, sh, sh + sep, gridTop, scrollY, viewH, &firstScene, &lastScene);

    // Track columns: only those intersecting the viewport.
    qint64 firstCol = 0, lastCol = -1;
    if (visibleRange(columns, sw + sep, sw + sep, 0, scrollX, viewW, &firstCol, &lastCol)) {
        for (qint64 t = firstCol; t <= lastCol; ++t) {
            const int x = static_cast<int>(t * (sw + sep) - scrollX);
            // Title bar filled with the track color, as in a session mixer.
            const QRect header(x, headerY, sw, hh);
            const int colorIndex = m_bridge->trackColorIndex(static_cast<quint64>(t));
            const QColor trackColor = m_theme->trackColor(colorIndex >= 0 ? colorIndex : static_cast<int>(t % 16));
            p.fillPath(paint::rounded(*m_theme, QRectF(header).adjusted(1, 1, -1, 0)), trackColor);
            if (t == m_selected) {
                p.setPen(QPen(m_theme->color(QStringLiteral("text.primary")), 2));
                p.setBrush(Qt::NoBrush);
                p.drawPath(paint::rounded(*m_theme, QRectF(header).adjusted(1.5, 1.5, -1.5, -0.5)));
            }
            p.setPen(m_theme->color(QStringLiteral("track.text")));
            p.drawText(header.adjusted(textInset, 0, -textInset, 0), Qt::AlignLeft | Qt::AlignVCenter,
                p.fontMetrics().elidedText(m_bridge->trackName(static_cast<quint64>(t)), Qt::ElideRight, sw - 2 * textInset));
            Q_UNUSED(band);
            Q_UNUSED(panel);
            Q_UNUSED(primary);
            Q_UNUSED(selection);
            for (qint64 s = firstScene; s <= lastScene; ++s) {
                const int y = static_cast<int>(gridTop + s * (sh + sep) - scrollY);
                const bool hovered = t == m_hoverTrack && s == m_hoverScene;
                const bool occupied = m_bridge->clipSlotOccupied(static_cast<quint64>(t), static_cast<quint64>(s));
                const QColor fill = occupied ? trackColor : hovered ? slotHover : slotColor;
                const QPainterPath slotShape = paint::rounded(*m_theme, QRectF(x + 1.5, y + 1.5, sw - 3, sh - 3));
                // Empty cells sit recessed; a clip reads as a raised block.
                paint::control(p, *m_theme, slotShape, fill, !occupied);
                // Launch button at the left edge of every slot: a triangle,
                // solid on a clip and dim on an empty slot. Launching itself
                // waits for a transport.
                const int triSize = qMax(5, sh / 2 - 1);
                const QPointF triOrigin(x + stopInset + 2, y + (sh - triSize) / 2.0);
                QPolygonF tri;
                tri << triOrigin << triOrigin + QPointF(triSize * 0.9, triSize / 2.0) << triOrigin + QPointF(0, triSize);
                p.setPen(Qt::NoPen);
                p.setBrush(occupied ? m_theme->color(QStringLiteral("track.text")) : stopColor);
                p.drawPolygon(tri);
                const int labelInset = stopInset + triSize + textInset + 2;
                if (occupied) {
                    p.setPen(m_theme->color(QStringLiteral("track.text")));
                    p.drawText(QRect(x + labelInset, y, sw - labelInset - textInset, sh), Qt::AlignLeft | Qt::AlignVCenter,
                        p.fontMetrics().elidedText(m_bridge->clipName(static_cast<quint64>(t), static_cast<quint64>(s)), Qt::ElideRight, sw - labelInset - textInset));
                }
                Q_UNUSED(stopSize);
            }
            Q_UNUSED(gridHeight);
            Q_UNUSED(sepColor);
        }
    }

    // Master column with scene launch slots.
    const qint64 masterX64 = qint64(columns) * (sw + sep) - scrollX;
    if (masterX64 < viewW && fitsCoordinate(masterX64)) {
        const int mx = static_cast<int>(masterX64);
        const int mw = masterWidth();
        const QRect header(mx, headerY, mw, hh);
        p.fillRect(header, panel);
        p.setPen(secondary);
        p.drawText(header.adjusted(textInset, band, -textInset, 0), Qt::AlignLeft | Qt::AlignVCenter, tr("Master"));
        const int labelInset = stopInset + qMax(5, sh / 2 - 1) + textInset + 2;
        for (qint64 s = firstScene; s <= lastScene; ++s) {
            const int y = static_cast<int>(gridTop + s * (sh + sep) - scrollY);
            const QRect slot(mx, y, mw, sh);
            p.fillPath(paint::rounded(*m_theme, QRectF(slot).adjusted(1, 1, -1, -1)), slotColor);
            p.setPen(secondary);
            p.drawText(slot.adjusted(labelInset, 0, -textInset, 0), Qt::AlignLeft | Qt::AlignVCenter,
                static_cast<quint64>(s) < m_bridge->sceneCount()
                    ? m_bridge->sceneName(static_cast<quint64>(s))
                    : QString::number(s + 1));
            // Scene launch triangle.
            const int triSize = qMax(5, sh / 2 - 1);
            const QPointF triOrigin(mx + stopInset + 2, y + (sh - triSize) / 2.0);
            QPolygonF tri;
            tri << triOrigin << triOrigin + QPointF(triSize * 0.9, triSize / 2.0) << triOrigin + QPointF(0, triSize);
            p.setPen(Qt::NoPen);
            p.setBrush(stopColor.lighter(140));
            p.drawPolygon(tri);
        }
        Q_UNUSED(headerY);
    }

    // Header underline across the visible width.
    p.fillRect(QRect(0, static_cast<int>(hh - scrollY), viewW, sep), sepColor);

    // Below the last scene, dim placeholder rows keep the grid readable
    // down to the bottom of the panel. They are not launchable slots.
    QColor ghost = slotColor;
    ghost.setAlpha(90);
    const qint64 filledBottom = gridTop + gridHeight - scrollY;
    if (columns > 0 && filledBottom < viewH) {
        const qint64 rowPitch = sh + sep;
        const qint64 firstGhost = scenes;
        const qint64 lastGhost = firstGhost + (viewH - filledBottom) / rowPitch + 1;
        for (qint64 t = firstCol; t <= lastCol; ++t) {
            const int x = static_cast<int>(t * (sw + sep) - scrollX);
            for (qint64 s = firstGhost; s <= lastGhost; ++s) {
                const int y = static_cast<int>(gridTop + s * rowPitch - scrollY);
                p.fillPath(paint::rounded(*m_theme, QRectF(x + 1, y + 1, sw - 2, sh - 2)), ghost);
            }
        }
    }
}

} // namespace nylon
