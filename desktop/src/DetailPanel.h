#pragma once

#include <QWidget>

class QLabel;
class QStackedWidget;

namespace nylon {

class FlatButton;
class Theme;

// Bottom panel: clip view or device chain for the selected track. The core
// exposes neither clips nor devices yet, so each page shows what it will
// hold and why it is empty.
class DetailPanel : public QWidget {
    Q_OBJECT
public:
    enum class Page { Clip, Device };

    explicit DetailPanel(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setSelectedTrack(int index, const QString& name);
    int selectedTrack() const { return m_track; }
    Page page() const { return m_page; }
    QString headerText() const;

public slots:
    void showPage(Page page);
    void showClipPage() { showPage(Page::Clip); }
    void showDevicePage() { showPage(Page::Device); }

private:
    void refresh();

    const Theme* m_theme;
    QLabel* m_title;
    FlatButton* m_clipTab;
    FlatButton* m_deviceTab;
    QStackedWidget* m_stack;
    QLabel* m_clipEmpty;
    QLabel* m_deviceEmpty;
    Page m_page = Page::Device;
    int m_track = -1;
    QString m_trackName;
};

} // namespace nylon
