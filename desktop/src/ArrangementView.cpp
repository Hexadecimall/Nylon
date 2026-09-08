#include "ArrangementView.h"

#include "LayoutMath.h"
#include "ProjectBridge.h"
#include "Theme.h"

#include <QPaintEvent>
#include <QPainter>
#include <QScrollBar>

namespace nylon {

using namespace layout;

ArrangementView::ArrangementView(ProjectBridge* bridge, const Theme* theme, QWidget* parent)
    : QAbstractScrollArea(parent)
    , m_bridge(bridge)
    , m_theme(theme)
{
    setObjectName(QStringLiteral("arrangement"));
    setFrameShape(QFrame::NoFrame);
    viewport()->setAutoFillBackground(false);
    connect(m_bridge, &ProjectBridge::changed, this, [this] {
        updateScrollRanges();
        viewport()->update();
    });
    updateScrollRanges();
}

void ArrangementView::setTheme(const Theme* theme)
{
    m_theme = theme;
    updateScrollRanges();
    viewport()->update();
}

int ArrangementView::separator() const
{
    return qMax(0, m_theme->metricInt(QStringLiteral("separator"), 1));
}

int ArrangementView::laneCount() const
{
    return static_cast<int>(layoutCount(static_cast<qint64>(m_bridge->trackCount()),
        laneHeight() + separator(), rulerHeight() + separator()));
}

int ArrangementView::laneHeight() const
{
    return qMax(16, m_theme->metricInt(QStringLiteral("arrangement.lane.height"), 56));
}

int ArrangementView::rulerHeight() const
{
    return qMax(12, m_theme->metricInt(QStringLiteral("arrangement.ruler.height"), 20));
}

int ArrangementView::headerWidth() const
{
    return qMax(40, m_theme->metricInt(QStringLiteral("arrangement.header.width"), 140));
}

int ArrangementView::pixelsPerBar() const
{
    return qMax(8, m_theme->metricInt(QStringLiteral("arrangement.pixels_per_bar"), 64));
}

int ArrangementView::barCount() const
{
    const qint64 requested = qMax(1, m_theme->metricInt(QStringLiteral("arrangement.bars"), 64));
    return static_cast<int>(layoutCount(requested, pixelsPerBar(), headerWidth() + separator()));
}

bool ArrangementView::isShowingEmptyState() const
{
    return m_bridge->trackCount() == 0;
}

QRect ArrangementView::laneRect(int track) const
{
    if (track < 0 || track >= laneCount()) {
        return QRect();
    }
    const qint64 sep = separator();
    const qint64 y = rulerHeight() + sep + qint64(track) * (laneHeight() + sep) - verticalScrollBar()->value();
    if (!fitsCoordinate(y)) {
        return QRect();
    }
    const int x = headerWidth() + static_cast<int>(sep);
    return QRect(x, static_cast<int>(y), qMax(0, viewport()->width() - x), laneHeight());
}

void ArrangementView::updateScrollRanges()
{
    const qint64 sep = separator();
    const qint64 contentW = headerWidth() + sep + qint64(barCount()) * pixelsPerBar();
    const qint64 contentH = rulerHeight() + sep + qint64(laneCount()) * (laneHeight() + sep);
    horizontalScrollBar()->setRange(0, clampExtent(contentW - viewport()->width()));
    horizontalScrollBar()->setPageStep(viewport()->width());
    verticalScrollBar()->setRange(0, clampExtent(contentH - viewport()->height()));
    verticalScrollBar()->setPageStep(viewport()->height());
}

void ArrangementView::resizeEvent(QResizeEvent* event)
{
    QAbstractScrollArea::resizeEvent(event);
    updateScrollRanges();
}

void ArrangementView::paintEvent(QPaintEvent* event)
{
    QPainter p(viewport());
    p.fillRect(event->rect(), m_theme->color(QStringLiteral("background")));
    p.setFont(font());

    const int sep = separator();
    const QColor sepColor = m_theme->color(QStringLiteral("separator"));
    const QColor panel = m_theme->color(QStringLiteral("panel"));
    const QColor ruler = m_theme->color(QStringLiteral("arrangement.ruler"));
    const QColor lane = m_theme->color(QStringLiteral("arrangement.lane"));
    const QColor laneAlt = m_theme->color(QStringLiteral("arrangement.lane.alt"));
    const QColor grid = m_theme->color(QStringLiteral("arrangement.grid"));
    const QColor gridBar = m_theme->color(QStringLiteral("arrangement.grid.bar"));
    const QColor primary = m_theme->color(QStringLiteral("text.primary"));
    const QColor secondary = m_theme->color(QStringLiteral("text.secondary"));
    const int textInset = m_theme->metricInt(QStringLiteral("text.inset"), 4);
    const int band = m_theme->metricInt(QStringLiteral("arrangement.header.band"), 3);

    const int lanes = laneCount();
    const int lh = laneHeight();
    const int rh = rulerHeight();
    const int hw = headerWidth();
    const int ppb = pixelsPerBar();
    const int bars = barCount();
    const qint64 scrollX = horizontalScrollBar()->value();
    const qint64 scrollY = verticalScrollBar()->value();
    const int viewW = viewport()->width();
    const int viewH = viewport()->height();
    const int timelineX = hw + sep;
    const qint64 lanesTop = rh + sep;

    qint64 firstLane = 0, lastLane = -1;
    const bool anyLane = visibleRange(lanes, lh, lh + sep, lanesTop, scrollY, viewH, &firstLane, &lastLane);
    qint64 firstBar = 0, lastBar = -1;
    const bool anyBar = visibleRange(bars, ppb, ppb, timelineX, scrollX, viewW, &firstBar, &lastBar);

    // Lane bodies.
    if (anyLane) {
        for (qint64 t = firstLane; t <= lastLane; ++t) {
            const int y = static_cast<int>(lanesTop + t * (lh + sep) - scrollY);
            p.fillRect(QRect(timelineX, y, viewW - timelineX, lh), (t % 2 == 0) ? lane : laneAlt);
            p.fillRect(QRect(0, y + lh, viewW, sep), sepColor);
        }
    }

    // Beat and bar grid over the visible lanes.
    if (anyLane && anyBar) {
        const int gridTop = static_cast<int>(qMax<qint64>(lanesTop - scrollY, lanesTop));
        const int gridBottom = static_cast<int>(qMin<qint64>(lanesTop + qint64(lanes) * (lh + sep) - scrollY, viewH));
        const int gridH = gridBottom - gridTop;
        for (qint64 b = firstBar; b <= lastBar + 1 && b <= bars; ++b) {
            const int x = static_cast<int>(timelineX + b * ppb - scrollX);
            if (x >= timelineX && x <= viewW) {
                p.fillRect(QRect(x, gridTop, sep, gridH), gridBar);
            }
            for (int beat = 1; beat < 4; ++beat) {
                const int bx = x + (beat * ppb) / 4;
                if (bx >= timelineX && bx <= viewW) {
                    p.fillRect(QRect(bx, gridTop, sep, gridH), grid);
                }
            }
        }
    }

    // Track headers, drawn after the grid so they cover scrolled content.
    if (anyLane) {
        for (qint64 t = firstLane; t <= lastLane; ++t) {
            const int y = static_cast<int>(lanesTop + t * (lh + sep) - scrollY);
            const QRect header(0, y, hw, lh);
            p.fillRect(header, panel);
            const int colorIndex = m_bridge->trackColorIndex(static_cast<quint64>(t));
            p.fillRect(QRect(0, y, band, lh),
                m_theme->trackColor(colorIndex >= 0 ? colorIndex : static_cast<int>(t % 16)));
            p.setPen(primary);
            p.drawText(header.adjusted(band + textInset, textInset, -textInset, 0), Qt::AlignLeft | Qt::AlignTop,
                p.fontMetrics().elidedText(m_bridge->trackName(static_cast<quint64>(t)), Qt::ElideRight,
                    hw - band - 2 * textInset));
            p.fillRect(QRect(hw, y, sep, lh), sepColor);
        }
    }

    // Ruler with bar numbers, pinned to the top.
    p.fillRect(QRect(0, 0, viewW, rh), ruler);
    p.fillRect(QRect(0, rh, viewW, sep), sepColor);
    p.setPen(secondary);
    if (anyBar) {
        for (qint64 b = firstBar; b <= lastBar; ++b) {
            const int x = static_cast<int>(timelineX + b * ppb - scrollX);
            p.fillRect(QRect(x, 0, sep, rh), sepColor);
            p.drawText(QRect(x + textInset, 0, ppb - textInset, rh), Qt::AlignLeft | Qt::AlignVCenter,
                QString::number(b + 1));
        }
    }
    p.fillRect(QRect(0, 0, hw, rh), panel);
    p.fillRect(QRect(hw, 0, sep, rh), sepColor);

    if (m_bridge->trackCount() == 0) {
        p.setPen(secondary);
        p.drawText(QRect(0, rh + sep, viewW, viewH - rh - sep), Qt::AlignCenter,
            tr("No tracks.\nAdd a track to start an arrangement."));
    }
}

} // namespace nylon
