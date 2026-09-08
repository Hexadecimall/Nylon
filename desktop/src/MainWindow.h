#pragma once

#include <QMainWindow>
#include <QSize>

class QAction;
class QActionGroup;
class QMenu;
class QLabel;
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
class TitleBar;
class TransportBar;
class FlatButton;

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
    TitleBar* titleBar() const { return m_titleBar; }

    bool isStartScreenVisible() const;
    bool isSessionVisible() const;
    bool isLowerDockVisible() const;
    QAction* action(const QString& objectName) const;
    // Every named action, for the command palette and shortcut editor.
    QList<QAction*> namedActions() const;

public slots:
    void showStartScreen();
    void newProject();
    void newProjectFromTemplate(int audioTracks, int midiTracks);
    void showSession();
    void showArrangement();
    void toggleView();
    void selectTrack(int index);
    void showPreferences();
    void showCommandPalette();
    void renameSelectedTrack();
    void deleteSelectedTrack();
    void toggleMaximized();
    // Bundle persistence. The dialog-free variants take a bundle directory
    // and return false when the core rejected it.
    void openProject();
    bool openProjectAt(const QString& bundleDirectory);
    bool saveProject();
    void saveProjectAs();
    bool saveProjectTo(const QString& bundleDirectory);

protected:
    void closeEvent(QCloseEvent* event) override;
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    bool eventFilter(QObject* watched, QEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;

private:
    void enterWorkspace();
    void buildMenus();
    void buildWorkspace();
    void applyTheme(const Theme& theme);
    void showStatus(const QString& text);
    void restoreLayout();
    void saveLayout();
    QString trackName(int index) const;
    void updateEditActions();
    void updateWindowTitle();
    void updateWorkspaceContext();
    void rebuildRecentMenu();
    void showLowerWidget(QWidget* widget);
    void hideLowerWidget(QWidget* widget);

    ProjectBridge* m_bridge;
    ThemeManager* m_themes;
    TitleBar* m_titleBar = nullptr;
    QStackedWidget* m_root;
    StartScreen* m_start;
    QWidget* m_workspace = nullptr;
    TransportBar* m_transport = nullptr;
    QSplitter* m_horizontal = nullptr;
    QSplitter* m_vertical = nullptr;
    BrowserPanel* m_browser = nullptr;
    QStackedWidget* m_views = nullptr;
    QStackedWidget* m_lowerViews = nullptr;
    QWidget* m_lowerDock = nullptr;
    SessionView* m_session = nullptr;
    MixerSection* m_mixer = nullptr;
    ArrangementView* m_arrangement = nullptr;
    DetailPanel* m_detail = nullptr;
    FlatButton* m_mixerTab = nullptr;
    FlatButton* m_detailTab = nullptr;
    FlatButton* m_lowerClose = nullptr;
    FlatButton* m_browserToggle = nullptr;
    FlatButton* m_mixerToggle = nullptr;
    FlatButton* m_editorToggle = nullptr;
    QLabel* m_workspaceContext = nullptr;
    QActionGroup* m_themeActions = nullptr;
    QMenu* m_recentMenu = nullptr;
    QSize m_workspaceSize{1440, 900};

    Qt::Edges edgesAt(const QPoint& pos) const;
    void updateResizeCursor(const QPoint& pos);
};

} // namespace nylon
