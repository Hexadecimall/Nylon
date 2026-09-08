#pragma once

#include <QWidget>

class QLabel;
class QStackedWidget;

namespace nylon {

class FlatButton;
class PianoRoll;
class ProjectBridge;
class Theme;

// Track inspector with clip and device pages.
class DetailPanel : public QWidget {
    Q_OBJECT
public:
    enum class Page { Clip, Device };

    DetailPanel(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setSelectedTrack(int index, const QString& name);
    int selectedTrack() const { return m_track; }
    // Shows the clip in `track`/`scene` on the Clip page; negative indices
    // clear the clip selection. The track selection follows `track`.
    void setSelectedClip(int track, int scene);
    int selectedClipTrack() const { return m_clipTrack; }
    int selectedClipScene() const { return m_clipScene; }
    PianoRoll* pianoRoll() const { return m_pianoRoll; }
    Page page() const { return m_page; }
    QString headerText() const;

public slots:
    void showPage(Page page);
    void showClipPage() { showPage(Page::Clip); }
    void showDevicePage() { showPage(Page::Device); }

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    void refresh();

    const Theme* m_theme;
    ProjectBridge* m_bridge;
    QLabel* m_title;
    QWidget* m_summary;
    QLabel* m_kind;
    QLabel* m_volume;
    QLabel* m_pan;
    QLabel* m_state;
    QLabel* m_color;
    FlatButton* m_clipTab;
    FlatButton* m_deviceTab;
    QStackedWidget* m_stack;
    QLabel* m_clipEmpty;
    QWidget* m_clipPage;
    QLabel* m_clipTitle;
    PianoRoll* m_pianoRoll;
    QLabel* m_deviceEmpty;
    int m_clipTrack = -1;
    int m_clipScene = -1;
    Page m_page = Page::Device;
    int m_track = -1;
    QString m_trackName;
};

} // namespace nylon
