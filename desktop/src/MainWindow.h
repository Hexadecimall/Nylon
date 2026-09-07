#pragma once

#include <QMainWindow>

class QStackedWidget;
class QActionGroup;

namespace nylon {

class ArrangementView;
class ProjectBridge;
class SessionView;
class Theme;
class ThemeManager;
class TransportBar;

class MainWindow : public QMainWindow {
    Q_OBJECT
public:
    explicit MainWindow(ProjectBridge* bridge, ThemeManager* themes, QWidget* parent = nullptr);

    ProjectBridge* bridge() const { return m_bridge; }
    TransportBar* transport() const { return m_transport; }
    SessionView* sessionView() const { return m_session; }
    ArrangementView* arrangementView() const { return m_arrangement; }
    bool isSessionVisible() const;

public slots:
    void showSession();
    void showArrangement();
    void toggleView();

private:
    void buildMenus();
    void applyTheme(const Theme& theme);
    void showStatus(const QString& text);

    ProjectBridge* m_bridge;
    ThemeManager* m_themes;
    TransportBar* m_transport;
    QStackedWidget* m_stack;
    SessionView* m_session;
    ArrangementView* m_arrangement;
    QActionGroup* m_themeActions = nullptr;
};

} // namespace nylon
