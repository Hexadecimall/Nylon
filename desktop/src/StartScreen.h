#pragma once

#include <QWidget>

class QLabel;
class QListWidget;

namespace nylon {

class FlatButton;
class Theme;

// First screen after launch: create a project, open one, or pick a recent
// one. Opening is enabled only when the core can read project bundles.
class StartScreen : public QWidget {
    Q_OBJECT
public:
    explicit StartScreen(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    // Enables Open and the recent list when the core supports persistence.
    void setPersistenceAvailable(bool available);
    bool isPersistenceAvailable() const { return m_persistence; }

    static QStringList recentProjects();
    static void addRecentProject(const QString& path);
    static void clearRecentProjects();

    FlatButton* newButton() const { return m_new; }
    FlatButton* openButton() const { return m_open; }
    QListWidget* recentList() const { return m_recent; }

public slots:
    void reloadRecent();

signals:
    void newProjectRequested();
    void openProjectRequested();
    void recentProjectRequested(const QString& path);

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    const Theme* m_theme;
    QLabel* m_title;
    QLabel* m_version;
    FlatButton* m_new;
    FlatButton* m_open;
    QLabel* m_recentTitle;
    QListWidget* m_recent;
    QLabel* m_recentEmpty;
    bool m_persistence = false;
};

} // namespace nylon
