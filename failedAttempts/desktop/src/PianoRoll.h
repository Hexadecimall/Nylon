#pragma once

#include "nylon.hpp"

#include <QAbstractScrollArea>

namespace nylon {

class ProjectBridge;
class Theme;

// MIDI clip editor: a keyboard down the left, a beat grid across, and the
// clip's notes as blocks. Notes are added by double-clicking an empty
// cell, removed with Delete or a double-click on a note, and moved by
// dragging. Every edit goes through the bridge, so it is one undo step.
class PianoRoll : public QAbstractScrollArea {
    Q_OBJECT
public:
    PianoRoll(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    // Selects the clip to edit; negative indices show the empty state.
    void setClip(qint64 track, qint64 scene);
    qint64 track() const { return m_track; }
    qint64 scene() const { return m_scene; }
    bool hasClip() const;

    // Grid geometry in pixels.
    int pixelsPerBeat() const { return m_pixelsPerBeat; }
    int rowHeight() const { return m_rowHeight; }
    void setZoom(int pixelsPerBeat);
    // Notes are quantized to this fraction of a beat when created or moved.
    double gridBeats() const { return m_gridBeats; }
    void setGridBeats(double beats);

    // Coordinate mapping in viewport space.
    QRect noteRect(const MidiNote& note) const;
    QPointF cellAt(const QPoint& pos) const; // x = beats, y = pitch (fractional)
    int noteIndexAt(const QPoint& pos) const;
    int selectedNote() const { return m_selected; }
    int keyboardWidth() const { return m_keyboardWidth; }
    int rulerHeight() const { return m_rulerHeight; }
    // Height of the velocity lane along the bottom edge.
    int velocityLaneHeight() const { return m_velocityHeight; }
    // Grip at the right edge of a note used to change its length.
    QRect noteResizeGrip(const MidiNote& note) const;
    // Velocity bar of a note in the lane, in viewport coordinates.
    QRect velocityBar(const MidiNote& note) const;

public slots:
    void selectNote(int index);
    void refresh();

signals:
    void noteSelected(int index);
    void message(const QString& text);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;

private:
    void updateScrollRanges();
    double loopBeats() const;
    double quantize(double beats) const;

    ProjectBridge* m_bridge;
    const Theme* m_theme;
    qint64 m_track = -1;
    qint64 m_scene = -1;
    int m_pixelsPerBeat = 48;
    int m_rowHeight = 12;
    int m_keyboardWidth = 56;
    int m_rulerHeight = 18;
    double m_gridBeats = 0.25;
    int m_selected = -1;
    enum class Drag { None, Move, Resize, Velocity };
    // Drag state.
    Drag m_drag = Drag::None;
    bool m_dragging = false;
    int m_velocityHeight = 48;
    QPoint m_dragStart;
    MidiNote m_dragOrigin{};
    MidiNote m_dragPreview{};
};

} // namespace nylon
