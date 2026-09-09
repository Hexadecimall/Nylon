#pragma once

#include <QAbstractScrollArea>
#include <QList>

namespace nylon {

class ProjectBridge;
class Theme;

// Linear timeline with core-backed track controls and clip placements.
class ArrangementView : public QAbstractScrollArea {
    Q_OBJECT
public:
    explicit ArrangementView(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);

    // Lanes as laid out, capped so the content extent stays within
    // layout::kMaxExtent.
    int laneCount() const;
    int barCount() const;
    // X coordinate of the start of a bar in viewport space, or -1 when the
    // bar is not laid out.
    int barX(int bar) const;
    // Beat lines drawn inside each bar, from the project's time signature.
    int beatsPerBar() const;
    // Rectangle of a lane body (excluding the header) in viewport
    // coordinates, or an empty rect when out of range.
    QRect laneRect(int track) const;
    // Rectangles inside a track header, in viewport coordinates. Empty
    // when the track is not laid out.
    QRect headerVolumeRect(int track) const;
    QRect headerNameRect(int track) const;
    // Mute, solo and record buttons, in that order.
    QRect headerStateRect(int track, int button) const;
    bool isShowingEmptyState() const;
    int selectedTrack() const { return m_selected; }
    double playheadBeats() const { return m_playheadBeats; }
    int playheadX() const;

public slots:
    void selectTrack(int track);
    void setPlayheadBeats(double beats);
    // Output level for one track in decibels, drawn as a strip down the
    // right edge of its header. Anything at or below the floor is silence.
    void setTrackLevel(int track, double peakDb);
    void clearTrackLevels();

signals:
    void trackSelected(int track);
    void locateRequested(double beats);
    // The row under the last track was clicked.
    void addTrackRequested();
    // A double-click on an empty lane asks for a clip of `lengthBeats`
    // starting at `startBeats` on `track`.
    void clipRequested(int track, double startBeats, double lengthBeats);

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;

private:
    void updateScrollRanges();
    int separator() const;
    int laneHeight() const;
    int rulerHeight() const;
    int headerWidth() const;
    int pixelsPerBar() const;
    int trackAt(int y) const;
    QRect headerRect(int track) const;
    // Row under the last track that offers to add another one.
    QRect addTrackRect() const;
    // Applies a volume drag at a point inside the header.
    void dragVolume(int track, int x);
    // Beat under a point in the timeline, clamped at zero.
    double beatsAt(int x) const;
    // Whether a point is in the ruler, where the playhead is dragged.
    bool inRuler(const QPoint& point) const;

    ProjectBridge* m_bridge;
    const Theme* m_theme;
    int m_selected = -1;
    // Peak level per track in decibels, as last reported.
    QList<double> m_levels;
    // Track whose volume slider is being dragged, or -1.
    int m_volumeDrag = -1;
    // True while the playhead is being dragged along the ruler.
    bool m_scrubbing = false;
    double m_playheadBeats = 0.0;
};

} // namespace nylon
