#include "SessionView.h"

#include "ProjectBridge.h"
#include "Theme.h"

#include <QPaintEvent>
#include <QPainter>
#include <QScrollBar>

namespace nylon {

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

int SessionView::columnCount() const
{
    return static_cast<int>(qMin<quint64>(m_bridge->trackCount(), 1u << 20));
}

int SessionView::sceneCount() const
{
    return qMax(1, m_theme->metricInt(QStringLiteral("session.scene.count"), 8));
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
    return columnCount() == 0;
}

QRect SessionView::slotRect(int track, int scene) const
{
    if (track < 0 || track >= columnCount() || scene < 0 || scene >= sceneCount()) {
        return QRect();
    }
    const int sep = m_theme->metricInt(QStringLiteral("separator"), 1);
    const int x = track * (slotWidth() + sep) - horizontalScrollBar()->value();
    const int y = headerHeight() + sep + scene * (slotHeight() + sep) - verticalScrollBar()->value();
    return QRect(x, y, slotWidth(), slotHeight());
}

void SessionView::updateScrollRanges()
{
    const int sep = m_theme->metricInt(QStringLiteral("separator"), 1);
    const int contentW = columnCount() * (slotWidth() + sep) + masterWidth();
    const int contentH = headerHeight() + sep + sceneCount() * (slotHeight() + sep);
    horizontalScrollBar()->setRange(0, qMax(0, contentW - viewport()->width()));
    horizontalScrollBar()->setPageStep(viewport()->width());
    verticalScrollBar()->setRange(0, qMax(0, contentH - viewport()->height()));
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

    const int sep = m_theme->metricInt(QStringLiteral("separator"), 1);
    const QColor sepColor = m_theme->color(QStringLiteral("separator"));
    const QColor slotColor = m_theme->color(QStringLiteral("session.slot"));
    const QColor stopColor = m_theme->color(QStringLiteral("session.stop_button"));
    const QColor panel = m_theme->color(QStringLiteral("panel"));
    const QColor primary = m_theme->color(QStringLiteral("text.primary"));
    const QColor secondary = m_theme->color(QStringLiteral("text.secondary"));
    QFont font = p.font();
    font.setPixelSize(m_theme->metricInt(QStringLiteral("font.size"), 11));
    p.setFont(font);

    const int columns = columnCount();
    const int scenes = sceneCount();
    const int sw = slotWidth();
    const int sh = slotHeight();
    const int hh = headerHeight();
    const int scrollX = horizontalScrollBar()->value();
    const int scrollY = verticalScrollBar()->value();
    const int viewW = viewport()->width();
    const int viewH = viewport()->height();

    if (columns == 0) {
        p.setPen(secondary);
        p.drawText(viewport()->rect(), Qt::AlignCenter,
            tr("No tracks.\nAdd a track to start a session."));
    }

    // Track columns.
    for (int t = 0; t < columns; ++t) {
        const int x = t * (sw + sep) - scrollX;
        if (x + sw < 0 || x > viewW) {
            continue;
        }
        // Header with the track color band and index.
        QRect header(x, -scrollY, sw, hh);
        p.fillRect(header, panel);
        p.fillRect(QRect(x, -scrollY, sw, 2), m_theme->trackColor(t));
        p.setPen(primary);
        p.drawText(header.adjusted(4, 2, -4, 0), Qt::AlignLeft | Qt::AlignVCenter,
            tr("%1").arg(t + 1));
        for (int s = 0; s < scenes; ++s) {
            const int y = hh + sep + s * (sh + sep) - scrollY;
            if (y + sh < 0 || y > viewH) {
                continue;
            }
            const QRect slot(x, y, sw, sh);
            p.fillRect(slot, slotColor);
            // Stop button marker at the left edge of each empty slot.
            p.fillRect(QRect(x + 3, y + sh / 2 - 2, 5, 5), stopColor);
        }
        p.fillRect(QRect(x + sw, -scrollY, sep, hh + sep + scenes * (sh + sep)), sepColor);
    }

    // Master column with scene launch slots.
    const int mx = columns * (sw + sep) - scrollX;
    if (mx < viewW) {
        const int mw = masterWidth();
        QRect header(mx, -scrollY, mw, hh);
        p.fillRect(header, panel);
        p.setPen(secondary);
        p.drawText(header.adjusted(4, 2, -4, 0), Qt::AlignLeft | Qt::AlignVCenter, tr("Master"));
        for (int s = 0; s < scenes; ++s) {
            const int y = hh + sep + s * (sh + sep) - scrollY;
            const QRect slot(mx, y, mw, sh);
            p.fillRect(slot, panel);
            p.setPen(secondary);
            p.drawText(slot.adjusted(10, 0, -4, 0), Qt::AlignLeft | Qt::AlignVCenter,
                tr("%1").arg(s + 1));
            p.fillRect(QRect(mx + 3, y + sh / 2 - 2, 4, 5), stopColor);
        }
        p.fillRect(QRect(mx - sep, -scrollY, sep, viewH + scrollY), sepColor);
    }

    // Row separators across the whole width.
    p.fillRect(QRect(0, hh - scrollY, viewW, sep), sepColor);
    for (int s = 0; s < scenes; ++s) {
        const int y = hh + sep + s * (sh + sep) + sh - scrollY;
        p.fillRect(QRect(0, y, viewW, sep), sepColor);
    }
}

} // namespace nylon
