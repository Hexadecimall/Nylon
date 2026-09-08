#pragma once

#include <QWidget>

class QLabel;
class QListWidget;

namespace nylon {

class FlatButton;
class Theme;

// Project launcher with a compact action rail and a recent-project workspace.
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
    FlatButton* recordingButton() const { return m_recording; }
    FlatButton* productionButton() const { return m_production; }
    FlatButton* openButton() const { return m_open; }
    QListWidget* recentList() const { return m_recent; }
    // The launcher card in widget coordinates.
    QRect cardRect() const;

public slots:
    void reloadRecent();

signals:
    void newProjectRequested();
    void templateRequested(int audioTracks, int midiTracks);
    void openProjectRequested();
    void recentProjectRequested(const QString& path);

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    const Theme* m_theme;
    QWidget* m_card;
    QWidget* m_sidebar;
    QWidget* m_recentPane;
    QLabel* m_title;
    QLabel* m_version;
    QLabel* m_tagline;
    FlatButton* m_new;
    FlatButton* m_recording;
    FlatButton* m_production;
    FlatButton* m_open;
    QLabel* m_recentTitle;
    QListWidget* m_recent;
    QLabel* m_recentEmpty;
    QLabel* m_shortcuts;
    QLabel* m_footer;
    bool m_persistence = false;
};

} // namespace nylon
