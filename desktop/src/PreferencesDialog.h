#pragma once

#include <QDialog>

class QComboBox;
class QLineEdit;
class QListWidget;
class QStackedWidget;
class QLabel;
class QAction;
class QTableWidget;

namespace nylon {

class ThemeManager;

// Preferences: Look & Feel (theme), Library (root folder), Audio (backend
// status). Changes apply immediately and persist through QSettings.
class PreferencesDialog : public QDialog {
    Q_OBJECT
public:
    PreferencesDialog(ThemeManager* themes, const QList<QAction*>& actions, QWidget* parent = nullptr);

    QComboBox* themeCombo() const { return m_theme; }
    QLineEdit* libraryField() const { return m_library; }
    QListWidget* sections() const { return m_sections; }
    QTableWidget* shortcutTable() const { return m_shortcuts; }
    // Applies a shortcut to the action in `row`; returns false and leaves
    // it unchanged when another action already uses the key.
    bool assignShortcut(int row, const QKeySequence& sequence);

signals:
    void libraryRootChanged(const QString& path);

private:
    void chooseLibraryFolder();

    void buildShortcutRows();

    ThemeManager* m_themes;
    QList<QAction*> m_actions;
    QListWidget* m_sections;
    QTableWidget* m_shortcuts = nullptr;
    QLabel* m_shortcutNote = nullptr;
    QStackedWidget* m_pages;
    QComboBox* m_theme;
    QLineEdit* m_library;
    QLabel* m_audioStatus;
};

} // namespace nylon
