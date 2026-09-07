#pragma once

#include <QAbstractScrollArea>

namespace nylon {

class ProjectBridge;
class Theme;

// Clip launch grid: one column per track, one row per scene, plus a master
// column at the right holding the scene launch slots. The core does not
// expose clips yet, so every slot is drawn empty.
class SessionView : public QAbstractScrollArea {
    Q_OBJECT
public:
    explicit SessionView(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);

    int columnCount() const;
    int sceneCount() const;
    // Rectangle of a slot in viewport coordinates, or an empty rect when the
    // indices are out of range.
    QRect slotRect(int track, int scene) const;
    bool isShowingEmptyState() const;

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;

private:
    void updateScrollRanges();
    int slotWidth() const;
    int slotHeight() const;
    int headerHeight() const;
    int masterWidth() const;

    ProjectBridge* m_bridge;
    const Theme* m_theme;
};

} // namespace nylon
