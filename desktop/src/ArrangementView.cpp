#include "ArrangementView.h"

#include "ProjectBridge.h"
#include "Theme.h"

#include <QPaintEvent>
#include <QPainter>
#include <QScrollBar>

namespace nylon {

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

int ArrangementView::laneCount() const
{
    return static_cast<int>(qMin<quint64>(m_bridge->trackCount(), 1u << 20));
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
    return qMax(1, m_theme->metricInt(QStringLiteral("arrangement.bars"), 64));
}

bool ArrangementView::isShowingEmptyState() const
{
    return laneCount() == 0;
}

QRect ArrangementView::laneRect(int track) const
{
    if (track < 0 || track >= laneCount()) {
        return QRect();
    }
    const int sep = m_theme->metricInt(QStringLiteral("separator"), 1);
    const int y = rulerHeight() + sep + track * (laneHeight() + sep) - verticalScrollBar()->value();
    const int x = headerWidth() + sep;
    return QRect(x, y, qMax(0, viewport()->width() - x), laneHeight());
}

void ArrangementView::updateScrollRanges()
{
    const int sep = m_theme->metricInt(QStringLiteral("separator"), 1);
    const int contentW = headerWidth() + sep + barCount() * pixelsPerBar();
    const int contentH = rulerHeight() + sep + laneCount() * (laneHeight() + sep);
    horizontalScrollBar()->setRange(0, qMax(0, contentW - viewport()->width()));
    horizontalScrollBar()->setPageStep(viewport()->width());
    verticalScrollBar()->setRange(0, qMax(0, contentH - viewport()->height()));
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

    const int sep = m_theme->metricInt(QStringLiteral("separator"), 1);
    const QColor sepColor = m_theme->color(QStringLiteral("separator"));
    const QColor panel = m_theme->color(QStringLiteral("panel"));
    const QColor ruler = m_theme->color(QStringLiteral("arrangement.ruler"));
    const QColor lane = m_theme->color(QStringLiteral("arrangement.lane"));
    const QColor laneAlt = m_theme->color(QStringLiteral("arrangement.lane.alt"));
    const QColor grid = m_theme->color(QStringLiteral("arrangement.grid"));
    const QColor gridBar = m_theme->color(QStringLiteral("arrangement.grid.bar"));
    const QColor primary = m_theme->color(QStringLiteral("text.primary"));
    const QColor secondary = m_theme->color(QStringLiteral("text.secondary"));
    QFont font = p.font();
    font.setPixelSize(m_theme->metricInt(QStringLiteral("font.size"), 11));
    p.setFont(font);

    const int lanes = laneCount();
    const int lh = laneHeight();
    const int rh = rulerHeight();
    const int hw = headerWidth();
    const int ppb = pixelsPerBar();
    const int bars = barCount();
    const int scrollX = horizontalScrollBar()->value();
    const int scrollY = verticalScrollBar()->value();
    const int viewW = viewport()->width();
    const int viewH = viewport()->height();
    const int timelineX = hw + sep;

    // Lane bodies and grid.
    for (int t = 0; t < lanes; ++t) {
        const int y = rh + sep + t * (lh + sep) - scrollY;
        if (y + lh < 0 || y > viewH) {
            continue;
        }
        p.fillRect(QRect(timelineX, y, viewW - timelineX, lh), (t % 2 == 0) ? lane : laneAlt);
        p.fillRect(QRect(0, y + lh, viewW, sep), sepColor);
    }
    const int lanesBottom = rh + sep + lanes * (lh + sep) - scrollY;
    if (lanes > 0) {
        for (int b = 0; b <= bars; ++b) {
            const int x = timelineX + b * ppb - scrollX;
            if (x < timelineX || x > viewW) {
                continue;
            }
            p.fillRect(QRect(x, rh + sep - scrollY, sep, lanesBottom - (rh + sep - scrollY)), gridBar);
            // Beat subdivisions.
            for (int beat = 1; beat < 4; ++beat) {
                const int bx = x + (beat * ppb) / 4;
                if (bx <= viewW) {
                    p.fillRect(QRect(bx, rh + sep - scrollY, sep, lanesBottom - (rh + sep - scrollY)), grid);
                }
            }
        }
    }

    // Track headers, drawn after the grid so they cover scrolled content.
    for (int t = 0; t < lanes; ++t) {
        const int y = rh + sep + t * (lh + sep) - scrollY;
        if (y + lh < 0 || y > viewH) {
            continue;
        }
        const QRect header(0, y, hw, lh);
        p.fillRect(header, panel);
        p.fillRect(QRect(0, y, 3, lh), m_theme->trackColor(t));
        p.setPen(primary);
        p.drawText(header.adjusted(8, 4, -4, 0), Qt::AlignLeft | Qt::AlignTop, tr("%1").arg(t + 1));
        p.fillRect(QRect(hw, y, sep, lh), sepColor);
    }

    // Ruler with bar numbers, pinned to the top.
    p.fillRect(QRect(0, 0, viewW, rh), ruler);
    p.fillRect(QRect(0, rh, viewW, sep), sepColor);
    p.setPen(secondary);
    for (int b = 0; b < bars; ++b) {
        const int x = timelineX + b * ppb - scrollX;
        if (x + ppb < timelineX || x > viewW) {
            continue;
        }
        p.fillRect(QRect(x, 0, sep, rh), sepColor);
        p.drawText(QRect(x + 3, 0, ppb - 3, rh), Qt::AlignLeft | Qt::AlignVCenter,
            QString::number(b + 1));
    }
    p.fillRect(QRect(0, 0, hw, rh), panel);
    p.fillRect(QRect(hw, 0, sep, rh), sepColor);

    if (lanes == 0) {
        p.setPen(secondary);
        p.drawText(QRect(0, rh + sep, viewW, viewH - rh - sep), Qt::AlignCenter,
            tr("No tracks.\nAdd a track to start an arrangement."));
    }
}

} // namespace nylon
