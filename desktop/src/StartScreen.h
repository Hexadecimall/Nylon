#pragma once

#include <QWidget>

class QLabel;
class QListWidget;

namespace nylon {

class FlatButton;
class Theme;

// First screen after launch: a centered launcher card with the actions to
// create or open a project and the recent list. The card sizes to its
// content; the surrounding canvas stays empty.
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
    // The launcher card in widget coordinates.
    QRect cardRect() const;

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
    QWidget* m_card;
    QLabel* m_title;
    QLabel* m_version;
    QLabel* m_tagline;
    FlatButton* m_new;
    FlatButton* m_open;
    QLabel* m_recentTitle;
    QListWidget* m_recent;
    QLabel* m_recentEmpty;
    QLabel* m_footer;
    bool m_persistence = false;
};

} // namespace nylon
