#pragma once

#include <QList>
#include <QPair>
#include <QString>
#include <QWidget>

class QLineEdit;
class QListWidget;
class QTreeView;
class QFileSystemModel;
class QSortFilterProxyModel;
class QLabel;

namespace nylon {

class Theme;

// Library browser: a category column on the left and the contents of the
// selected category folder on the right. Categories map to folders under
// the library root, which is created on first use.
class BrowserPanel : public QWidget {
    Q_OBJECT
public:
    explicit BrowserPanel(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);

    // Root folder of the library. Read from settings; defaults to a
    // "Nylon Library" folder in the platform music location.
    static QString libraryRoot();
    static void setLibraryRoot(const QString& path);
    // Category names in display order.
    static QStringList categories();
    // Folder name used for a category under the library root.
    static QString folderForCategory(const QString& category);

    QString currentCategory() const;
    QString currentFolder() const;
    bool isShowingEmptyState() const;
    int visibleEntryCount() const;

    QLineEdit* searchField() const { return m_search; }
    QListWidget* categoryList() const { return m_categories; }
    QTreeView* tree() const { return m_tree; }

public slots:
    void selectCategory(const QString& category);
    void reload();

signals:
    // A file was activated (double-click or Return).
    void fileActivated(const QString& path);
    void selectionChanged(const QString& path);
    // A category was double-clicked, which asks for it in its own window.
    void categoryDetached(const QString& category);

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    void ensureLibraryLayout();
    void updateEmptyState();
    void onCategoryChanged();
    // Points the tree at a folder and updates the location line.
    void showFolder(const QString& path, const QString& label);

    const Theme* m_theme;
    QLineEdit* m_search;
    QListWidget* m_categories;
    QTreeView* m_tree;
    QFileSystemModel* m_model;
    QSortFilterProxyModel* m_proxy;
    QLabel* m_empty;
    QLabel* m_info;
};

} // namespace nylon
