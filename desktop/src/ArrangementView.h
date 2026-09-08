#pragma once

#include <QAbstractScrollArea>

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
    bool isShowingEmptyState() const;
    int selectedTrack() const { return m_selected; }
    double playheadBeats() const { return m_playheadBeats; }
    int playheadX() const;

public slots:
    void selectTrack(int track);
    void setPlayheadBeats(double beats);

signals:
    void trackSelected(int track);

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;

private:
    void updateScrollRanges();
    int separator() const;
    int laneHeight() const;
    int rulerHeight() const;
    int headerWidth() const;
    int pixelsPerBar() const;
    int trackAt(int y) const;
    QRect headerButtonRect(int track, int button) const;

    ProjectBridge* m_bridge;
    const Theme* m_theme;
    int m_selected = -1;
    double m_playheadBeats = 0.0;
};

} // namespace nylon
