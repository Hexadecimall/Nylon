#pragma once

#include <QAbstractScrollArea>

namespace nylon {

class ProjectBridge;
class Theme;

// Clip launch grid: one column per track, one row per scene, plus a master
// column at the right holding the scene launch slots.
class SessionView : public QAbstractScrollArea {
    Q_OBJECT
public:
    explicit SessionView(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);

    // Counts as laid out. Both are capped so the content extent stays
    // within layout::kMaxExtent; the project's own track count is unbounded.
    int columnCount() const;
    int sceneCount() const;
    // Rectangle of a slot in viewport coordinates, or an empty rect when the
    // indices are out of range.
    QRect slotRect(int track, int scene) const;
    bool isShowingEmptyState() const;
    int selectedTrack() const { return m_selected; }
    // Column index under an x coordinate in viewport space, or -1.
    int columnAt(int x) const;
    int sceneAt(int y) const;

public slots:
    void selectTrack(int index);

signals:
    void trackSelected(int index);
    void slotClicked(int track, int scene);
    void slotCreateRequested(int track, int scene);

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;

private:
    void updateScrollRanges();
    int separator() const;
    int slotWidth() const;
    int slotHeight() const;
    int headerHeight() const;
    int masterWidth() const;

    ProjectBridge* m_bridge;
    const Theme* m_theme;
    int m_selected = -1;
    int m_hoverTrack = -1;
    int m_hoverScene = -1;
};

} // namespace nylon
