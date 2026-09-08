#pragma once

#include <QMainWindow>

class QAction;
class QActionGroup;
class QMenu;
class QSplitter;
class QStackedWidget;

namespace nylon {

class ArrangementView;
class BrowserPanel;
class DetailPanel;
class MixerSection;
class ProjectBridge;
class SessionView;
class StartScreen;
class Theme;
class ThemeManager;
class TransportBar;

class MainWindow : public QMainWindow {
    Q_OBJECT
public:
    explicit MainWindow(ProjectBridge* bridge, ThemeManager* themes, QWidget* parent = nullptr);
    ~MainWindow() override;

    ProjectBridge* bridge() const { return m_bridge; }
    TransportBar* transport() const { return m_transport; }
    SessionView* sessionView() const { return m_session; }
    ArrangementView* arrangementView() const { return m_arrangement; }
    BrowserPanel* browser() const { return m_browser; }
    DetailPanel* detail() const { return m_detail; }
    MixerSection* mixer() const { return m_mixer; }
    StartScreen* startScreen() const { return m_start; }

    bool isStartScreenVisible() const;
    bool isSessionVisible() const;
    QAction* action(const QString& objectName) const;

public slots:
    void showStartScreen();
    void newProject();
    void showSession();
    void showArrangement();
    void toggleView();
    void selectTrack(int index);
    void showPreferences();
    void renameSelectedTrack();
    void deleteSelectedTrack();

protected:
    void closeEvent(QCloseEvent* event) override;

private:
    void buildMenus();
    void buildWorkspace();
    void applyTheme(const Theme& theme);
    void showStatus(const QString& text);
    void restoreLayout();
    void saveLayout();
    QString trackName(int index) const;
    void updateEditActions();

    ProjectBridge* m_bridge;
    ThemeManager* m_themes;
    QStackedWidget* m_root;
    StartScreen* m_start;
    QWidget* m_workspace = nullptr;
    TransportBar* m_transport = nullptr;
    QSplitter* m_horizontal = nullptr;
    QSplitter* m_vertical = nullptr;
    BrowserPanel* m_browser = nullptr;
    QStackedWidget* m_views = nullptr;
    SessionView* m_session = nullptr;
    MixerSection* m_mixer = nullptr;
    ArrangementView* m_arrangement = nullptr;
    DetailPanel* m_detail = nullptr;
    QActionGroup* m_themeActions = nullptr;
    QMenu* m_recentMenu = nullptr;
};

} // namespace nylon
