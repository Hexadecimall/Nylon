#pragma once

#include <QDialog>

class QLineEdit;
class QListWidget;
class QAction;

namespace nylon {

class Theme;

// Search box over every named action in the window. Typing filters by
// action text; Enter triggers the highlighted action. Disabled actions
// are listed dimmed and cannot be triggered.
class CommandPalette : public QDialog {
    Q_OBJECT
public:
    CommandPalette(const QList<QAction*>& actions, const Theme* theme, QWidget* parent = nullptr);

    // Actions listed for the current filter text, in display order.
    QList<QAction*> visibleActions() const;
    QAction* currentAction() const;
    QLineEdit* searchField() const { return m_search; }
    void setFilter(const QString& text);

    // True when `pattern` matches `text` as an ordered subsequence,
    // case-insensitively.
    static bool fuzzyMatch(const QString& pattern, const QString& text);
    // Lower is better; ranks contiguous and prefix matches first.
    static int matchScore(const QString& pattern, const QString& text);

signals:
    void triggered(QAction* action);

protected:
    void keyPressEvent(QKeyEvent* event) override;
    void paintEvent(QPaintEvent* event) override;

private:
    void rebuild();
    void activateCurrent();

    QList<QAction*> m_actions;
    const Theme* m_theme;
    QLineEdit* m_search;
    QListWidget* m_list;
};

} // namespace nylon
