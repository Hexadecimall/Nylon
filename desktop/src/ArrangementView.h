#pragma once

#include <QAbstractScrollArea>

namespace nylon {

class ProjectBridge;
class Theme;

// Linear timeline: a bar ruler across the top and one lane per track with a
// fixed-width header at the left. Clips and automation are not exposed by
// the core yet, so lanes are drawn empty.
class ArrangementView : public QAbstractScrollArea {
    Q_OBJECT
public:
    explicit ArrangementView(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);

    // Lanes as laid out, capped so the content extent stays within
    // layout::kMaxExtent.
    int laneCount() const;
    int barCount() const;
    // Rectangle of a lane body (excluding the header) in viewport
    // coordinates, or an empty rect when out of range.
    QRect laneRect(int track) const;
    bool isShowingEmptyState() const;

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;

private:
    void updateScrollRanges();
    int separator() const;
    int laneHeight() const;
    int rulerHeight() const;
    int headerWidth() const;
    int pixelsPerBar() const;

    ProjectBridge* m_bridge;
    const Theme* m_theme;
};

} // namespace nylon
