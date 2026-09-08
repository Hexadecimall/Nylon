#include "ArrangementView.h"

#include "LayoutMath.h"
#include "PanelPaint.h"
#include "ProjectBridge.h"
#include "Theme.h"
#include "widgets/Fader.h"

#include <QPaintEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QScrollBar>

#include <cmath>
#include <limits>

namespace nylon {

using namespace layout;

namespace {
// Track header geometry. A header reads as two rows: name and kind on
// top, state buttons and level below.
constexpr int kHeaderMargin = 7;
constexpr int kHeaderGap = 5;
constexpr int kHeaderBandLeft = 6;
constexpr int kHeaderBandWidth = 12;
constexpr int kHeaderBadgeWidth = 52;
constexpr int kHeaderReadoutWidth = 54;
constexpr int kHeaderSliderMinimum = 24;
// Alpha of the wash over the selected lane and over every other bar.
constexpr int kSelectedLaneWash = 12;
constexpr int kAlternateBarWash = 60;
// The volume slider spans the same range a mixer fader does.
constexpr double kHeaderMinimumDb = -70.0;
constexpr double kHeaderMaximumDb = 6.0;
const double kInfinity = std::numeric_limits<double>::infinity();
} // namespace

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

int ArrangementView::beatsPerBar() const
{
    return qBound(1, m_bridge->timeSignatureNumerator(), 64);
}

int ArrangementView::barX(int bar) const
{
    if (bar < 0 || bar >= barCount()) {
        return -1;
    }
    const qint64 x = headerWidth() + separator() + qint64(bar) * pixelsPerBar() - horizontalScrollBar()->value();
    return fitsCoordinate(x) ? static_cast<int>(x) : -1;
}

int ArrangementView::playheadX() const
{
    const double scale = static_cast<double>(pixelsPerBar()) / static_cast<double>(beatsPerBar());
    const double x = static_cast<double>(headerWidth() + separator()) + m_playheadBeats * scale
        - static_cast<double>(horizontalScrollBar()->value());
    return x >= static_cast<double>(std::numeric_limits<int>::min())
            && x <= static_cast<double>(std::numeric_limits<int>::max())
        ? static_cast<int>(std::lround(x))
        : -1;
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

int ArrangementView::trackAt(int y) const
{
    const qint64 content = qint64(y) + verticalScrollBar()->value() - rulerHeight() - separator();
    if (content < 0) return -1;
    const qint64 pitch = laneHeight() + separator();
    const qint64 track = content / pitch;
    if (track >= laneCount() || content - track * pitch >= laneHeight()) return -1;
    return static_cast<int>(track);
}

QRect ArrangementView::headerRect(int track) const
{
    if (track < 0 || track >= laneCount()) {
        return QRect();
    }
    const int y = rulerHeight() + separator() + track * (laneHeight() + separator())
        - verticalScrollBar()->value();
    return QRect(0, y, headerWidth(), laneHeight());
}

QRect ArrangementView::headerStateRect(int track, int button) const
{
    const QRect header = headerRect(track);
    if (header.isNull() || button < 0 || button > 2) {
        return QRect();
    }
    // The controls sit on a second row under the name, the way a track
    // header reads in an arrangement: name and kind above, state and
    // level below.
    const int size = qBound(12, header.height() / 3, 20);
    const int top = header.bottom() - size - kHeaderMargin + 1;
    return QRect(kHeaderBandWidth + kHeaderMargin + button * (size + kHeaderGap), top, size, size);
}

QRect ArrangementView::headerNameRect(int track) const
{
    const QRect header = headerRect(track);
    if (header.isNull()) {
        return QRect();
    }
    const int left = kHeaderBandWidth + kHeaderMargin;
    const int height = qBound(14, header.height() / 3, 24);
    return QRect(left, header.top() + kHeaderMargin, qMax(0, header.width() - left - kHeaderBadgeWidth - kHeaderMargin),
        height);
}

QRect ArrangementView::headerVolumeRect(int track) const
{
    const QRect header = headerRect(track);
    const QRect last = headerStateRect(track, 2);
    if (header.isNull() || last.isNull()) {
        return QRect();
    }
    const int left = last.right() + kHeaderGap + 1;
    const int right = header.right() - kHeaderMargin - kHeaderReadoutWidth;
    if (right - left < kHeaderSliderMinimum) {
        return QRect();
    }
    const int height = qMax(4, last.height() / 3);
    return QRect(left, last.center().y() - height / 2 + 1, right - left, height);
}

void ArrangementView::dragVolume(int track, int x)
{
    const QRect slider = headerVolumeRect(track);
    if (slider.isNull() || slider.width() <= 0) {
        return;
    }
    const double position = qBound(0.0, static_cast<double>(x - slider.left()) / slider.width(), 1.0);
    const double db = Fader::decibelsForPosition(position, kHeaderMinimumDb, kHeaderMaximumDb);
    m_bridge->setTrackVolumeDb(static_cast<quint64>(track), db <= kHeaderMinimumDb ? -kInfinity : db);
    viewport()->update();
}

void ArrangementView::selectTrack(int track)
{
    track = qBound(-1, track, laneCount() - 1);
    if (m_selected == track) return;
    m_selected = track;
    viewport()->update();
}

void ArrangementView::setPlayheadBeats(double beats)
{
    const double next = std::isfinite(beats) ? qMax(0.0, beats) : 0.0;
    if (qFuzzyCompare(m_playheadBeats + 1.0, next + 1.0)) return;
    m_playheadBeats = next;
    viewport()->update();
}

void ArrangementView::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        QAbstractScrollArea::mousePressEvent(event);
        return;
    }
    const QPoint point = event->position().toPoint();
    if (point.y() < rulerHeight() && point.x() >= headerWidth() + separator()) {
        const double scale = static_cast<double>(pixelsPerBar()) / static_cast<double>(beatsPerBar());
        const double beats = (static_cast<double>(point.x() + horizontalScrollBar()->value()
                                  - headerWidth() - separator()))
            / scale;
        setPlayheadBeats(beats);
        emit locateRequested(m_playheadBeats);
        event->accept();
        return;
    }
    const int track = trackAt(point.y());
    if (track < 0) return;
    selectTrack(track);
    emit trackSelected(track);
    if (point.x() < headerWidth()) {
        const quint64 index = static_cast<quint64>(track);
        const QRect slider = headerVolumeRect(track);
        if (headerStateRect(track, 0).contains(point)) {
            m_bridge->setTrackMuted(index, !m_bridge->trackMuted(index));
        } else if (headerStateRect(track, 1).contains(point)) {
            m_bridge->setTrackSolo(index, !m_bridge->trackSolo(index));
        } else if (headerStateRect(track, 2).contains(point)) {
            m_bridge->setTrackArmed(index, !m_bridge->trackArmed(index));
        } else if (!slider.isNull() && slider.adjusted(0, -6, 0, 6).contains(point)) {
            m_volumeDrag = track;
            dragVolume(track, point.x());
        }
    }
    event->accept();
}

void ArrangementView::mouseMoveEvent(QMouseEvent* event)
{
    if (m_volumeDrag < 0) {
        QAbstractScrollArea::mouseMoveEvent(event);
        return;
    }
    dragVolume(m_volumeDrag, event->position().toPoint().x());
    event->accept();
}

void ArrangementView::mouseReleaseEvent(QMouseEvent* event)
{
    if (m_volumeDrag < 0) {
        QAbstractScrollArea::mouseReleaseEvent(event);
        return;
    }
    m_volumeDrag = -1;
    event->accept();
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
    paint::panel(p, *m_theme, viewport()->rect(), m_theme->color(QStringLiteral("panel")));
    p.setClipPath(paint::clip(*m_theme, viewport()->rect().adjusted(1, 1, -1, -1)));
    p.setFont(font());
    Q_UNUSED(event);

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
    // Beat subdivisions follow the project's time signature.
    const int beatsPerBar = qBound(1, m_bridge->timeSignatureNumerator(), 64);

    // Lane bodies. The selected lane takes a wash of its own track colour
    // so the row being edited is obvious without a border around it.
    if (anyLane) {
        for (qint64 t = firstLane; t <= lastLane; ++t) {
            const int y = static_cast<int>(lanesTop + t * (lh + sep) - scrollY);
            const QRect body(timelineX, y, viewW - timelineX, lh);
            p.fillRect(body, (t % 2 == 0) ? lane : laneAlt);
            if (static_cast<int>(t) == m_selected) {
                const int colorIndex = m_bridge->trackColorIndex(static_cast<quint64>(t));
                QColor wash = m_theme->trackColor(colorIndex >= 0 ? colorIndex : static_cast<int>(t % 16));
                wash.setAlpha(kSelectedLaneWash);
                p.fillRect(body, wash);
            }
            p.fillRect(QRect(0, y + lh, viewW, sep), sepColor);
        }
    }

    // Every other bar carries a faint wash, which gives the timeline a
    // sense of distance that plain grid lines do not.
    if (anyLane && anyBar) {
        QColor shade = m_theme->color(QStringLiteral("background"));
        shade.setAlpha(kAlternateBarWash);
        const int top = static_cast<int>(qMax<qint64>(lanesTop - scrollY, lanesTop));
        for (qint64 b = firstBar; b <= lastBar && b < bars; ++b) {
            if (b % 2 == 0) {
                continue;
            }
            const int x = static_cast<int>(timelineX + b * ppb - scrollX);
            const int left = qMax(x, timelineX);
            const int right = qMin(x + ppb, viewW);
            if (right > left) {
                p.fillRect(QRect(left, top, right - left, viewH - top), shade);
            }
        }
    }

    // Beat and bar grid over the visible lanes.
    if (anyLane && anyBar) {
        const int gridTop = static_cast<int>(qMax<qint64>(lanesTop - scrollY, lanesTop));
        const int gridBottom = viewH;
        const int gridH = gridBottom - gridTop;
        for (qint64 b = firstBar; b <= lastBar + 1 && b <= bars; ++b) {
            const int x = static_cast<int>(timelineX + b * ppb - scrollX);
            if (x >= timelineX && x <= viewW) {
                p.fillRect(QRect(x, gridTop, sep, gridH), gridBar);
            }
            for (int beat = 1; beat < beatsPerBar; ++beat) {
                const int bx = x + (beat * ppb) / beatsPerBar;
                if (bx >= timelineX && bx <= viewW) {
                    p.fillRect(QRect(bx, gridTop, sep, gridH), grid);
                }
            }
        }
    }

    // Continue horizontal structure below the last live track so the editor
    // remains a timeline at every window size.
    const qint64 liveBottom = lanesTop + qint64(lanes) * (lh + sep) - scrollY;
    if (liveBottom < viewH) {
        const int guideHeight = qMax(28, lh / 2);
        QColor guide = sepColor;
        guide.setAlpha(120);
        for (int y = static_cast<int>(liveBottom); y < viewH; y += guideHeight) {
            p.fillRect(QRect(0, y, viewW, sep), guide);
        }
    }


    // Arrangement clips sit above the grid and retain the source clip color.
    if (anyLane) {
        for (qint64 t = firstLane; t <= lastLane; ++t) {
            const int y = static_cast<int>(lanesTop + t * (lh + sep) - scrollY);
            const quint64 count = m_bridge->arrangementClipCount(static_cast<quint64>(t));
            for (quint64 index = 0; index < count; ++index) {
                BeatRange range {};
                if (!m_bridge->arrangementClipRange(static_cast<quint64>(t), index, range)) continue;
                const double scale = static_cast<double>(ppb) / static_cast<double>(beatsPerBar);
                const double x = static_cast<double>(timelineX) + range.startBeats * scale - static_cast<double>(scrollX);
                const double width = qMax(3.0, range.lengthBeats * scale);
                if (x + width < timelineX || x > viewW) continue;
                const int colorIndex = m_bridge->arrangementClipColorIndex(static_cast<quint64>(t), index);
                const QColor clipColor = m_theme->trackColor(colorIndex >= 0 ? colorIndex : static_cast<int>(t % 16));
                const QRectF clipRect(x + 2.0, y + 5.0, width - 4.0, lh - 10.0);
                p.fillPath(paint::rounded(*m_theme, clipRect), clipColor);
                p.setPen(m_theme->color(QStringLiteral("track.text")));
                p.drawText(clipRect.adjusted(textInset, 0, -textInset, 0), Qt::AlignLeft | Qt::AlignTop,
                    p.fontMetrics().elidedText(m_bridge->arrangementClipName(static_cast<quint64>(t), index),
                        Qt::ElideRight, qMax(0, qRound(clipRect.width()) - 2 * textInset)));
            }
        }
    }

    // Track headers, drawn after the grid so they cover scrolled content.
    if (anyLane) {
        const QColor raised = m_theme->color(QStringLiteral("raised"));
        const QColor control = m_theme->color(QStringLiteral("control.background"));
        for (qint64 t = firstLane; t <= lastLane; ++t) {
            const int track = static_cast<int>(t);
            const quint64 index = static_cast<quint64>(t);
            const QRect header = headerRect(track);
            const bool selected = track == m_selected;
            p.fillRect(header, selected ? raised : panel);
            const int colorIndex = m_bridge->trackColorIndex(index);
            const QColor trackColor = m_theme->trackColor(colorIndex >= 0 ? colorIndex : track % 16);
            // The colour band names the track at a glance, and widens on
            // the selected track so the selection reads from across the
            // window.
            const int bandWidth = selected ? band + 3 : band;
            p.fillPath(paint::rounded(*m_theme,
                           QRectF(kHeaderBandLeft, header.top() + 6, bandWidth, header.height() - 12)),
                trackColor);

            const QRect name = headerNameRect(track);
            // The number keeps a row identifiable when names repeat.
            const QRect number(name.left(), name.top(), kHeaderBadgeWidth / 2, name.height());
            p.setPen(secondary);
            p.drawText(number, Qt::AlignLeft | Qt::AlignVCenter, QString::number(track + 1));
            const int numberWidth = p.fontMetrics().horizontalAdvance(QStringLiteral("00")) + kHeaderGap;
            const QRect title = name.adjusted(numberWidth, 0, 0, 0);
            p.setPen(primary);
            p.drawText(title, Qt::AlignLeft | Qt::AlignVCenter,
                p.fontMetrics().elidedText(m_bridge->trackName(index), Qt::ElideRight, title.width()));

            // Kind badge, right aligned on the first row.
            const QRect badge(header.right() - kHeaderBadgeWidth - kHeaderMargin, name.top(),
                kHeaderBadgeWidth, name.height());
            p.fillPath(paint::rounded(*m_theme, QRectF(badge)), control);
            p.setPen(secondary);
            p.drawText(badge, Qt::AlignCenter,
                ProjectBridge::kindName(m_bridge->trackKind(index)).toUpper());

            const bool states[] = {m_bridge->trackMuted(index), m_bridge->trackSolo(index),
                m_bridge->trackArmed(index)};
            const QString labels[] = {tr("M"), tr("S"), tr("R")};
            const QString keys[] = {QStringLiteral("state.mute"), QStringLiteral("state.solo"),
                QStringLiteral("state.arm")};
            for (int button = 0; button < 3; ++button) {
                const QRect rect = headerStateRect(track, button);
                p.fillPath(paint::rounded(*m_theme, QRectF(rect)),
                    m_theme->color(states[button] ? keys[button] : QStringLiteral("control.background")));
                p.setPen(states[button] ? m_theme->color(QStringLiteral("accent.text")) : secondary);
                p.drawText(rect, Qt::AlignCenter, labels[button]);
            }

            // Volume, as a slider that can be dragged rather than a number
            // that can only be read.
            const double db = m_bridge->trackVolumeDb(index);
            const QRect slider = headerVolumeRect(track);
            if (!slider.isNull()) {
                p.fillPath(paint::rounded(*m_theme, QRectF(slider)), control);
                const double position = std::isinf(db) && db < 0.0
                    ? 0.0
                    : Fader::positionForDecibels(db, kHeaderMinimumDb, kHeaderMaximumDb);
                const int filled = qRound(position * slider.width());
                if (filled > 0) {
                    QColor fill = trackColor;
                    fill.setAlpha(selected ? 255 : 200);
                    p.fillPath(paint::rounded(*m_theme,
                                   QRectF(slider.left(), slider.top(), filled, slider.height())),
                        fill);
                }
                const int handleX = slider.left() + filled;
                p.fillPath(paint::rounded(*m_theme,
                               QRectF(handleX - 2, slider.top() - 4, 4, slider.height() + 8)),
                    primary);
            }
            p.setPen(secondary);
            const QRect readout(header.right() - kHeaderReadoutWidth - kHeaderMargin + 2,
                headerStateRect(track, 0).top(), kHeaderReadoutWidth, headerStateRect(track, 0).height());
            p.drawText(readout, Qt::AlignRight | Qt::AlignVCenter,
                std::isinf(db) ? tr("-inf") : tr("%1 dB").arg(db, 0, 'f', 1));

            p.fillRect(QRect(hw, header.top(), sep, header.height()), sepColor);
        }
    }

    // Ruler with bar numbers, pinned to the top.
    p.fillRect(QRect(0, 0, viewW, rh), ruler);
    p.fillRect(QRect(0, rh, viewW, sep), sepColor);
    if (anyBar) {
        // Bars carry a full tick and a number; beats carry a short tick,
        // which is what makes a ruler readable while zoomed out.
        const int barTick = qMax(4, rh / 3);
        const int beatTick = qMax(2, rh / 6);
        for (qint64 b = firstBar; b <= lastBar; ++b) {
            const int x = static_cast<int>(timelineX + b * ppb - scrollX);
            p.fillRect(QRect(x, rh - barTick, sep, barTick), gridBar);
            for (int beat = 1; beat < beatsPerBar; ++beat) {
                const int bx = x + (beat * ppb) / beatsPerBar;
                if (bx >= timelineX && bx <= viewW) {
                    p.fillRect(QRect(bx, rh - beatTick, sep, beatTick), grid);
                }
            }
            p.setPen(secondary);
            p.drawText(QRect(x + textInset, 0, ppb - textInset, rh - beatTick), Qt::AlignLeft | Qt::AlignVCenter,
                QString::number(b + 1));
        }
    }
    p.fillRect(QRect(0, 0, hw, rh), panel);
    p.fillRect(QRect(hw, 0, sep, rh), sepColor);

    const int cursorX = playheadX();
    if (cursorX >= timelineX && cursorX <= viewW) {
        const QColor playhead = m_theme->color(QStringLiteral("playhead"));
        p.fillRect(QRect(cursorX, rh - 5, 2, qMax(0, viewH - rh + 5)), playhead);
        QPolygonF marker;
        marker << QPointF(cursorX - 4, rh - 6) << QPointF(cursorX + 6, rh - 6) << QPointF(cursorX + 1, rh);
        p.setPen(Qt::NoPen);
        p.setBrush(playhead);
        p.drawPolygon(marker);
    }

    if (m_bridge->trackCount() == 0) {
        p.setPen(secondary);
        p.drawText(QRect(0, rh + sep, viewW, viewH - rh - sep), Qt::AlignCenter,
            tr("No tracks.\nAdd a track to start an arrangement."));
    }
}

} // namespace nylon
