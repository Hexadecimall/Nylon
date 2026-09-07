#include "SessionView.h"

#include "LayoutMath.h"
#include "ProjectBridge.h"
#include "Theme.h"

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
    connect(m_bridge, &ProjectBridge::changed, this, [this] {
        updateScrollRanges();
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
    const qint64 requested = qMax(1, m_theme->metricInt(QStringLiteral("session.scene.count"), 8));
    return static_cast<int>(layoutCount(requested, slotHeight() + separator(), headerHeight() + separator()));
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
    return m_theme->metricInt(QStringLiteral("control.height"), 20) + 2;
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
    p.fillRect(event->rect(), m_theme->color(QStringLiteral("background")));
    p.setFont(font());

    const int sep = separator();
    const QColor sepColor = m_theme->color(QStringLiteral("separator"));
    const QColor slotColor = m_theme->color(QStringLiteral("session.slot"));
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
            const QRect header(x, headerY, sw, hh);
            p.fillRect(header, panel);
            p.fillRect(QRect(x, headerY, sw, band), m_theme->trackColor(static_cast<int>(t % 16)));
            p.setPen(primary);
            p.drawText(header.adjusted(textInset, band, -textInset, 0), Qt::AlignLeft | Qt::AlignVCenter,
                QString::number(t + 1));
            for (qint64 s = firstScene; s <= lastScene; ++s) {
                const int y = static_cast<int>(gridTop + s * (sh + sep) - scrollY);
                p.fillRect(QRect(x, y, sw, sh), slotColor);
                p.fillRect(QRect(x + stopInset, y + (sh - stopSize) / 2, stopSize, stopSize), stopColor);
            }
            const int columnBottom = static_cast<int>(qMin<qint64>(gridTop + gridHeight - scrollY, viewH));
            p.fillRect(QRect(x + sw, headerY, sep, columnBottom - headerY), sepColor);
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
        const int labelInset = stopInset + stopSize + textInset;
        for (qint64 s = firstScene; s <= lastScene; ++s) {
            const int y = static_cast<int>(gridTop + s * (sh + sep) - scrollY);
            const QRect slot(mx, y, mw, sh);
            p.fillRect(slot, panel);
            p.setPen(secondary);
            p.drawText(slot.adjusted(labelInset, 0, -textInset, 0), Qt::AlignLeft | Qt::AlignVCenter,
                QString::number(s + 1));
            p.fillRect(QRect(mx + stopInset, y + (sh - stopSize) / 2, stopSize, stopSize), stopColor);
        }
        p.fillRect(QRect(mx - sep, headerY, sep, viewH - headerY), sepColor);
    }

    // Row separators across the visible width.
    p.fillRect(QRect(0, static_cast<int>(hh - scrollY), viewW, sep), sepColor);
    for (qint64 s = firstScene; s <= lastScene; ++s) {
        const int y = static_cast<int>(gridTop + s * (sh + sep) + sh - scrollY);
        p.fillRect(QRect(0, y, viewW, sep), sepColor);
    }
}

} // namespace nylon
