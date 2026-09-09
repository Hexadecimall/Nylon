#pragma once

#include <QWidget>

class QScrollBar;

namespace nylon {

class MixerStrip;
class ProjectBridge;
class Theme;

// Row of channel strips under the session grid, one per track plus a
// master strip pinned at the right. Horizontal scrolling follows the grid.
class MixerSection : public QWidget {
    Q_OBJECT
public:
    MixerSection(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    // Scrollbar whose value offsets the strips (the session grid's).
    void followScrollBar(QScrollBar* bar);

    int stripCount() const { return static_cast<int>(m_strips.size()); }
    MixerStrip* strip(int index) const { return m_strips.value(index); }
    MixerStrip* masterStrip() const { return m_master; }
    int selectedTrack() const { return m_selected; }

    QSize sizeHint() const override;

public slots:
    void selectTrack(int index);
    void rebuild();
    void setTrackLevels(int index, double peakLeftDb, double peakRightDb, double rmsLeftDb, double rmsRightDb);
    void setMasterLevels(double peakLeftDb, double peakRightDb, double rmsLeftDb, double rmsRightDb);
    void clearClipping();

signals:
    void trackSelected(int index);

protected:
    void resizeEvent(QResizeEvent* event) override;
    void paintEvent(QPaintEvent* event) override;

private:
    void relayout();
    // Copies the core's state for one track into its strip without
    // emitting edit signals.
    void syncStrip(int index);

    ProjectBridge* m_bridge;
    const Theme* m_theme;
    QWidget* m_stripHost;
    QList<MixerStrip*> m_strips;
    MixerStrip* m_master;
    QScrollBar* m_scroll = nullptr;
    int m_selected = -1;
};

} // namespace nylon
