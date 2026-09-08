#pragma once

#include <QDialog>

class QComboBox;
class QLineEdit;
class QListWidget;
class QStackedWidget;
class QLabel;

namespace nylon {

class ThemeManager;

// Preferences: Look & Feel (theme), Library (root folder), Audio (backend
// status). Changes apply immediately and persist through QSettings.
class PreferencesDialog : public QDialog {
    Q_OBJECT
public:
    explicit PreferencesDialog(ThemeManager* themes, QWidget* parent = nullptr);

    QComboBox* themeCombo() const { return m_theme; }
    QLineEdit* libraryField() const { return m_library; }
    QListWidget* sections() const { return m_sections; }

signals:
    void libraryRootChanged(const QString& path);

private:
    void chooseLibraryFolder();

    ThemeManager* m_themes;
    QListWidget* m_sections;
    QStackedWidget* m_pages;
    QComboBox* m_theme;
    QLineEdit* m_library;
    QLabel* m_audioStatus;
};

} // namespace nylon
