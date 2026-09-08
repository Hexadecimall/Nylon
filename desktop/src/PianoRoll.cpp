#include "PianoRoll.h"

#include "PanelPaint.h"
#include "ProjectBridge.h"
#include "Theme.h"

#include <QKeyEvent>
#include <QMouseEvent>
#include <QPaintEvent>
#include <QPainter>
#include <QScrollBar>

#include <cmath>

namespace nylon {

namespace {
constexpr int kPitchCount = 128;
const char* const kNoteNames[] = {"C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"};

bool isBlackKey(int pitch)
{
    const int n = pitch % 12;
    return n == 1 || n == 3 || n == 6 || n == 8 || n == 10;
}
} // namespace

PianoRoll::PianoRoll(ProjectBridge* bridge, const Theme* theme, QWidget* parent)
    : QAbstractScrollArea(parent)
    , m_bridge(bridge)
    , m_theme(theme)
{
    setObjectName(QStringLiteral("pianoRoll"));
    setFrameShape(QFrame::NoFrame);
    setFocusPolicy(Qt::StrongFocus);
    viewport()->setAutoFillBackground(false);
    viewport()->setMouseTracking(true);
    connect(m_bridge, &ProjectBridge::changed, this, &PianoRoll::refresh);
    updateScrollRanges();
    // Start scrolled to the middle of the keyboard.
    verticalScrollBar()->setValue((kPitchCount - 72) * m_rowHeight);
}

void PianoRoll::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_rowHeight = qMax(8, theme->metricInt(QStringLiteral("font.size"), 11) + 2);
    updateScrollRanges();
    viewport()->update();
}

void PianoRoll::setClip(qint64 track, qint64 scene)
{
    m_track = track;
    m_scene = scene;
    m_selected = -1;
    updateScrollRanges();
    viewport()->update();
}

bool PianoRoll::hasClip() const
{
    return m_track >= 0 && m_scene >= 0
        && m_bridge->clipSlotOccupied(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
}

void PianoRoll::setZoom(int pixelsPerBeat)
{
    m_pixelsPerBeat = qBound(8, pixelsPerBeat, 400);
    updateScrollRanges();
    viewport()->update();
}

void PianoRoll::setGridBeats(double beats)
{
    if (beats > 0.0) {
        m_gridBeats = beats;
        viewport()->update();
    }
}

double PianoRoll::loopBeats() const
{
    if (!hasClip()) {
        return 16.0;
    }
    const BeatRange r = m_bridge->clipLoop(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
    return qMax(1.0, r.startBeats + r.lengthBeats);
}

double PianoRoll::quantize(double beats) const
{
    return std::round(beats / m_gridBeats) * m_gridBeats;
}

void PianoRoll::updateScrollRanges()
{
    const int contentW = m_keyboardWidth + static_cast<int>(std::ceil((loopBeats() + 4.0) * m_pixelsPerBeat));
    const int contentH = m_rulerHeight + kPitchCount * m_rowHeight;
    const int noteArea = qMax(1, viewport()->height() - m_velocityHeight);
    horizontalScrollBar()->setRange(0, qMax(0, contentW - viewport()->width()));
    horizontalScrollBar()->setPageStep(viewport()->width());
    verticalScrollBar()->setRange(0, qMax(0, contentH - noteArea));
    verticalScrollBar()->setPageStep(noteArea);
}

QRect PianoRoll::noteRect(const MidiNote& note) const
{
    const int x = m_keyboardWidth + static_cast<int>(std::lround(note.startBeats * m_pixelsPerBeat)) - horizontalScrollBar()->value();
    const int w = qMax(3, static_cast<int>(std::lround(note.lengthBeats * m_pixelsPerBeat)) - 1);
    const int row = (kPitchCount - 1) - note.pitch;
    const int y = m_rulerHeight + row * m_rowHeight - verticalScrollBar()->value();
    return QRect(x, y + 1, w, m_rowHeight - 2);
}

QRect PianoRoll::noteResizeGrip(const MidiNote& note) const
{
    const QRect r = noteRect(note);
    const int grip = qMin(8, qMax(3, r.width() / 3));
    return QRect(r.right() - grip + 1, r.y(), grip, r.height());
}

QRect PianoRoll::velocityBar(const MidiNote& note) const
{
    const QRect r = noteRect(note);
    const int laneTop = viewport()->height() - m_velocityHeight;
    const int h = qMax(1, (m_velocityHeight - 6) * note.velocity / 127);
    return QRect(r.x(), laneTop + m_velocityHeight - 3 - h, qMax(3, qMin(r.width(), 6)), h);
}

QPointF PianoRoll::cellAt(const QPoint& pos) const
{
    const double beats = static_cast<double>(pos.x() - m_keyboardWidth + horizontalScrollBar()->value()) / m_pixelsPerBeat;
    const double row = static_cast<double>(pos.y() - m_rulerHeight + verticalScrollBar()->value()) / m_rowHeight;
    return QPointF(beats, (kPitchCount - 1) - std::floor(row));
}

int PianoRoll::noteIndexAt(const QPoint& pos) const
{
    if (!hasClip()) {
        return -1;
    }
    const quint64 n = m_bridge->clipNoteCount(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
    for (quint64 i = n; i > 0; --i) {
        MidiNote note{};
        if (m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), i - 1, note)
            && noteRect(note).contains(pos)) {
            return static_cast<int>(i - 1);
        }
    }
    return -1;
}

void PianoRoll::selectNote(int index)
{
    if (index == m_selected) {
        return;
    }
    m_selected = index;
    viewport()->update();
    emit noteSelected(index);
}

void PianoRoll::refresh()
{
    if (hasClip()) {
        const quint64 n = m_bridge->clipNoteCount(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
        if (m_selected >= static_cast<int>(n)) {
            m_selected = -1;
        }
    } else {
        m_selected = -1;
    }
    updateScrollRanges();
    viewport()->update();
}

void PianoRoll::resizeEvent(QResizeEvent* event)
{
    QAbstractScrollArea::resizeEvent(event);
    updateScrollRanges();
}

void PianoRoll::mousePressEvent(QMouseEvent* event)
{
    setFocus();
    if (event->button() != Qt::LeftButton || !hasClip()) {
        return;
    }
    // Velocity lane: press on a note's bar starts a velocity drag.
    if (event->pos().y() >= viewport()->height() - m_velocityHeight) {
        const quint64 n = m_bridge->clipNoteCount(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
        for (quint64 i = n; i > 0; --i) {
            MidiNote note{};
            if (m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), i - 1, note)
                && velocityBar(note).adjusted(-2, -6, 2, 0).contains(event->pos())) {
                selectNote(static_cast<int>(i - 1));
                m_drag = Drag::Velocity;
                m_dragging = true;
                m_dragStart = event->pos();
                m_dragOrigin = note;
                m_dragPreview = note;
                return;
            }
        }
        return;
    }
    const int index = noteIndexAt(event->pos());
    selectNote(index);
    if (index >= 0) {
        MidiNote note{};
        m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), static_cast<quint64>(index), note);
        m_drag = noteResizeGrip(note).contains(event->pos()) ? Drag::Resize : Drag::Move;
        m_dragging = true;
        m_dragStart = event->pos();
        m_dragOrigin = note;
        m_dragPreview = note;
    }
}

void PianoRoll::mouseMoveEvent(QMouseEvent* event)
{
    if (!m_dragging) {
        // Cursor hints: resize grip on hover.
        const int index = noteIndexAt(event->pos());
        if (index >= 0) {
            MidiNote note{};
            m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), static_cast<quint64>(index), note);
            viewport()->setCursor(noteResizeGrip(note).contains(event->pos()) ? Qt::SizeHorCursor : Qt::ArrowCursor);
        } else {
            viewport()->setCursor(Qt::ArrowCursor);
        }
        return;
    }
    const double dBeats = static_cast<double>(event->pos().x() - m_dragStart.x()) / m_pixelsPerBeat;
    const int dRows = static_cast<int>(std::lround(static_cast<double>(event->pos().y() - m_dragStart.y()) / m_rowHeight));
    m_dragPreview = m_dragOrigin;
    switch (m_drag) {
    case Drag::Move:
        m_dragPreview.startBeats = qMax(0.0, quantize(m_dragOrigin.startBeats + dBeats));
        m_dragPreview.pitch = static_cast<std::uint8_t>(qBound(0, static_cast<int>(m_dragOrigin.pitch) - dRows, kPitchCount - 1));
        break;
    case Drag::Resize:
        m_dragPreview.lengthBeats = qMax(m_gridBeats, quantize(m_dragOrigin.lengthBeats + dBeats));
        break;
    case Drag::Velocity: {
        const int delta = m_dragStart.y() - event->pos().y();
        m_dragPreview.velocity = static_cast<std::uint8_t>(qBound(1, static_cast<int>(m_dragOrigin.velocity) + delta, 127));
        break;
    }
    case Drag::None:
        break;
    }
    viewport()->update();
}

void PianoRoll::mouseReleaseEvent(QMouseEvent* event)
{
    if (!m_dragging || event->button() != Qt::LeftButton) {
        return;
    }
    m_dragging = false;
    m_drag = Drag::None;
    const bool changed = m_dragPreview.startBeats != m_dragOrigin.startBeats || m_dragPreview.pitch != m_dragOrigin.pitch
        || m_dragPreview.lengthBeats != m_dragOrigin.lengthBeats || m_dragPreview.velocity != m_dragOrigin.velocity;
    if (changed) {
        if (!m_bridge->moveClipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene),
                static_cast<quint64>(m_selected), m_dragPreview)) {
            emit message(tr("The core rejected that edit."));
        }
    }
    viewport()->update();
}

void PianoRoll::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton || !hasClip()) {
        return;
    }
    const int index = noteIndexAt(event->pos());
    if (index >= 0) {
        if (m_bridge->removeClipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), static_cast<quint64>(index))) {
            selectNote(-1);
        }
        return;
    }
    if (event->pos().x() < m_keyboardWidth || event->pos().y() < m_rulerHeight
        || event->pos().y() >= viewport()->height() - m_velocityHeight) {
        return;
    }
    const QPointF cell = cellAt(event->pos());
    if (cell.x() < 0.0 || cell.y() < 0.0 || cell.y() >= kPitchCount) {
        return;
    }
    MidiNote note{};
    note.pitch = static_cast<std::uint8_t>(cell.y());
    note.velocity = 100;
    note.startBeats = std::floor(cell.x() / m_gridBeats) * m_gridBeats;
    note.lengthBeats = m_gridBeats;
    if (m_bridge->addClipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), note)) {
        const quint64 n = m_bridge->clipNoteCount(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
        // Find the note just added to select it.
        for (quint64 i = 0; i < n; ++i) {
            MidiNote probe{};
            if (m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), i, probe)
                && probe.pitch == note.pitch && probe.startBeats == note.startBeats) {
                selectNote(static_cast<int>(i));
                break;
            }
        }
    } else {
        emit message(tr("The core rejected that note."));
    }
}

void PianoRoll::wheelEvent(QWheelEvent* event)
{
    if (event->modifiers() & Qt::ControlModifier) {
        const int steps = event->angleDelta().y() / 120;
        if (steps != 0) {
            setZoom(m_pixelsPerBeat + steps * 8);
        }
        event->accept();
        return;
    }
    QAbstractScrollArea::wheelEvent(event);
}

void PianoRoll::keyPressEvent(QKeyEvent* event)
{
    if ((event->key() == Qt::Key_Delete || event->key() == Qt::Key_Backspace) && m_selected >= 0 && hasClip()) {
        if (m_bridge->removeClipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), static_cast<quint64>(m_selected))) {
            selectNote(-1);
        }
        return;
    }
    QAbstractScrollArea::keyPressEvent(event);
}

void PianoRoll::paintEvent(QPaintEvent*)
{
    QPainter p(viewport());
    const QRect view = viewport()->rect();
    paint::panel(p, *m_theme, view, m_theme->color(QStringLiteral("panel")));
    p.setClipPath(paint::clip(*m_theme, view.adjusted(1, 1, -1, -1)));
    p.setFont(font());

    const QColor lane = m_theme->color(QStringLiteral("arrangement.lane"));
    const QColor laneAlt = m_theme->color(QStringLiteral("arrangement.lane.alt"));
    const QColor grid = m_theme->color(QStringLiteral("arrangement.grid"));
    const QColor gridBar = m_theme->color(QStringLiteral("arrangement.grid.bar"));
    const QColor secondary = m_theme->color(QStringLiteral("text.secondary"));
    const QColor primary = m_theme->color(QStringLiteral("text.primary"));

    if (!hasClip()) {
        p.setPen(secondary);
        p.drawText(view, Qt::AlignCenter, tr("No clip selected.\nDouble-click an empty slot to create a MIDI clip."));
        return;
    }

    const int scrollX = horizontalScrollBar()->value();
    const int scrollY = verticalScrollBar()->value();
    const int beatsPerBar = qBound(1, m_bridge->timeSignatureNumerator(), 64);
    const int laneTop = view.height() - m_velocityHeight;
    p.save();
    p.setClipRect(QRect(0, 0, view.width(), laneTop));
    const double loopEnd = loopBeats();
    const int gridLeft = m_keyboardWidth;

    // Rows: white/black key shading.
    const int firstRow = qMax(0, scrollY / m_rowHeight);
    const int lastRow = qMin(kPitchCount - 1, (scrollY + view.height()) / m_rowHeight + 1);
    for (int row = firstRow; row <= lastRow; ++row) {
        const int pitch = (kPitchCount - 1) - row;
        const int y = m_rulerHeight + row * m_rowHeight - scrollY;
        p.fillRect(QRect(gridLeft, y, view.width() - gridLeft, m_rowHeight), isBlackKey(pitch) ? laneAlt : lane);
        if (pitch % 12 == 0) {
            p.fillRect(QRect(gridLeft, y + m_rowHeight - 1, view.width() - gridLeft, 1), gridBar);
        }
    }

    // Columns: grid, beat, and bar lines; the loop end is marked.
    const double firstBeat = qMax(0.0, static_cast<double>(scrollX) / m_pixelsPerBeat);
    const double lastBeat = static_cast<double>(scrollX + view.width()) / m_pixelsPerBeat;
    for (double b = std::floor(firstBeat / m_gridBeats) * m_gridBeats; b <= lastBeat; b += m_gridBeats) {
        const int x = gridLeft + static_cast<int>(std::lround(b * m_pixelsPerBeat)) - scrollX;
        if (x < gridLeft) {
            continue;
        }
        const long beatIndex = std::lround(b);
        const bool onBeat = std::abs(b - beatIndex) < 1e-9;
        const bool onBar = onBeat && beatIndex % beatsPerBar == 0;
        p.fillRect(QRect(x, m_rulerHeight, 1, view.height()), onBar ? gridBar : (onBeat ? grid : grid.darker(115)));
    }
    const int loopX = gridLeft + static_cast<int>(std::lround(loopEnd * m_pixelsPerBeat)) - scrollX;
    p.fillRect(QRect(loopX, m_rulerHeight, view.width(), view.height()), QColor(0, 0, 0, 70));
    p.fillRect(QRect(loopX, 0, 2, view.height()), m_theme->color(QStringLiteral("state.loop")));

    // Notes.
    const quint64 count = m_bridge->clipNoteCount(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
    const int colorIndex = m_bridge->clipColorIndex(static_cast<quint64>(m_track), static_cast<quint64>(m_scene));
    const QColor clipColor = m_theme->trackColor(colorIndex >= 0 ? colorIndex : static_cast<int>(m_track % 16));
    p.setRenderHint(QPainter::Antialiasing, true);
    for (quint64 i = 0; i < count; ++i) {
        MidiNote note{};
        if (!m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), i, note)) {
            continue;
        }
        const bool selected = static_cast<int>(i) == m_selected;
        if (selected && m_dragging) {
            note = m_dragPreview;
        }
        const QRect r = noteRect(note);
        QColor fill = clipColor;
        fill.setAlpha(120 + note.velocity);
        p.fillPath(paint::rounded(*m_theme, QRectF(r)), fill);
        if (selected) {
            p.setPen(QPen(primary, 1.5));
            p.setBrush(Qt::NoBrush);
            p.drawPath(paint::rounded(*m_theme, QRectF(r).adjusted(0.5, 0.5, -0.5, -0.5)));
            const QRect grip = noteResizeGrip(note);
            p.fillRect(QRect(grip.x(), grip.y() + 2, 1, grip.height() - 4), primary);
        }
    }
    p.setRenderHint(QPainter::Antialiasing, false);

    p.restore();
    // Keyboard, pinned at the left.
    for (int row = firstRow; row <= lastRow; ++row) {
        const int pitch = (kPitchCount - 1) - row;
        const int y = m_rulerHeight + row * m_rowHeight - scrollY;
        const bool black = isBlackKey(pitch);
        p.fillRect(QRect(0, y, m_keyboardWidth, m_rowHeight), black ? QColor(30, 31, 33) : QColor(226, 228, 230));
        p.fillRect(QRect(0, y + m_rowHeight - 1, m_keyboardWidth, 1), QColor(0, 0, 0, 80));
        if (pitch % 12 == 0) {
            p.setPen(QColor(40, 42, 45));
            p.drawText(QRect(4, y, m_keyboardWidth - 8, m_rowHeight), Qt::AlignLeft | Qt::AlignVCenter,
                QStringLiteral("%1%2").arg(QLatin1String(kNoteNames[0])).arg(pitch / 12 - 2));
        }
    }
    p.fillRect(QRect(m_keyboardWidth - 1, 0, 1, view.height()), gridBar);

    // Ruler with bar numbers, pinned at the top.
    p.fillRect(QRect(0, 0, view.width(), m_rulerHeight), m_theme->color(QStringLiteral("arrangement.ruler")));
    p.fillRect(QRect(0, m_rulerHeight - 1, view.width(), 1), gridBar);
    p.setPen(secondary);
    for (long bar = 0;; ++bar) {
        const double beat = static_cast<double>(bar * beatsPerBar);
        const int x = gridLeft + static_cast<int>(std::lround(beat * m_pixelsPerBeat)) - scrollX;
        if (x > view.width()) {
            break;
        }
        if (x >= gridLeft) {
            p.fillRect(QRect(x, 0, 1, m_rulerHeight), gridBar);
            p.drawText(QRect(x + 3, 0, beatsPerBar * m_pixelsPerBeat, m_rulerHeight), Qt::AlignLeft | Qt::AlignVCenter,
                QString::number(bar + 1));
        }
    }
    p.fillRect(QRect(0, 0, m_keyboardWidth, m_rulerHeight), m_theme->color(QStringLiteral("panel")));
    p.setPen(secondary);
    p.drawText(QRect(4, 0, m_keyboardWidth - 4, m_rulerHeight), Qt::AlignLeft | Qt::AlignVCenter,
        m_bridge->clipName(static_cast<quint64>(m_track), static_cast<quint64>(m_scene)).left(6));

    // Velocity lane along the bottom: one bar per note, aligned to its start.
    p.fillRect(QRect(0, laneTop, view.width(), m_velocityHeight), m_theme->color(QStringLiteral("arrangement.ruler")));
    p.fillRect(QRect(0, laneTop, view.width(), 1), gridBar);
    p.setPen(secondary);
    p.drawText(QRect(4, laneTop, m_keyboardWidth - 4, m_velocityHeight), Qt::AlignLeft | Qt::AlignVCenter, tr("Vel"));
    for (quint64 i = 0; i < count; ++i) {
        MidiNote note{};
        if (!m_bridge->clipNote(static_cast<quint64>(m_track), static_cast<quint64>(m_scene), i, note)) {
            continue;
        }
        const bool selected = static_cast<int>(i) == m_selected;
        if (selected && m_dragging) {
            note = m_dragPreview;
        }
        const QRect bar = velocityBar(note);
        if (bar.x() < m_keyboardWidth) {
            continue;
        }
        p.fillRect(bar, selected ? primary : clipColor);
    }
}

} // namespace nylon
