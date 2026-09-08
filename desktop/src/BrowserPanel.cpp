#include "BrowserPanel.h"

#include "PanelPaint.h"
#include "Theme.h"

#include <QPaintEvent>
#include <QPainter>

#include <QDir>
#include <QFileInfo>
#include <QFileSystemModel>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QSettings>
#include <QSortFilterProxyModel>
#include <QStandardPaths>
#include <QTimer>
#include <QTreeView>
#include <QVBoxLayout>

namespace nylon {

namespace {

const char* const kSettingsKey = "library/root";

struct Category {
    const char* name;
    const char* folder;
};

const Category kCategories[] = {
    {"Sounds", "Sounds"},
    {"Drums", "Drums"},
    {"Instruments", "Instruments"},
    {"Audio Effects", "Audio Effects"},
    {"MIDI Effects", "MIDI Effects"},
    {"Plug-Ins", "Plug-Ins"},
    {"Clips", "Clips"},
    {"Samples", "Samples"},
    {"Grooves", "Grooves"},
};

} // namespace

BrowserPanel::BrowserPanel(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_search(new QLineEdit(this))
    , m_categories(new QListWidget(this))
    , m_tree(new QTreeView(this))
    , m_model(new QFileSystemModel(this))
    , m_proxy(new QSortFilterProxyModel(this))
    , m_empty(new QLabel(this))
    , m_info(new QLabel(this))
{
    setObjectName(QStringLiteral("browser"));
    setAutoFillBackground(false);

    m_search->setObjectName(QStringLiteral("browserSearch"));
    m_search->setPlaceholderText(tr("Search (Ctrl+F)"));
    m_search->setClearButtonEnabled(true);
    m_search->setStatusTip(tr("Filter the current category by name."));

    m_categories->setObjectName(QStringLiteral("browserCategories"));
    m_categories->setFrameShape(QFrame::NoFrame);
    m_categories->setSelectionMode(QAbstractItemView::SingleSelection);
    m_categories->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_categories->setVerticalScrollBarPolicy(Qt::ScrollBarAsNeeded);
    m_categories->setTextElideMode(Qt::ElideRight);
    m_categories->setStatusTip(tr("Library categories. Each one is a folder in the library."));
    for (const Category& c : kCategories) {
        m_categories->addItem(QString::fromLatin1(c.name));
    }

    m_model->setReadOnly(true);
    m_model->setNameFilterDisables(false);
    m_model->setFilter(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::AllDirs);
    m_proxy->setSourceModel(m_model);
    m_proxy->setRecursiveFilteringEnabled(true);
    m_proxy->setFilterCaseSensitivity(Qt::CaseInsensitive);
    m_proxy->setFilterKeyColumn(0);

    m_tree->setObjectName(QStringLiteral("browserTree"));
    m_tree->setModel(m_proxy);
    m_tree->setFrameShape(QFrame::NoFrame);
    m_tree->setHeaderHidden(true);
    m_tree->setRootIsDecorated(true);
    m_tree->setUniformRowHeights(true);
    m_tree->setSelectionMode(QAbstractItemView::SingleSelection);
    m_tree->setDragEnabled(true);
    m_tree->setStatusTip(tr("Contents of the selected category. Double-click a file to load it."));
    for (int col = 1; col < 4; ++col) {
        m_tree->hideColumn(col);
    }

    m_empty->setObjectName(QStringLiteral("secondary"));
    m_empty->setAlignment(Qt::AlignCenter);
    m_empty->setWordWrap(true);
    m_empty->setMargin(12);

    m_info->setObjectName(QStringLiteral("secondary"));
    m_info->setMargin(4);

    auto* right = new QWidget(this);
    auto* rightLayout = new QVBoxLayout(right);
    rightLayout->setContentsMargins(0, 0, 0, 0);
    rightLayout->setSpacing(0);
    rightLayout->addWidget(m_tree, 1);
    rightLayout->addWidget(m_empty, 1);
    rightLayout->addWidget(m_info);

    auto* columns = new QHBoxLayout;
    columns->setContentsMargins(0, 0, 0, 0);
    columns->setSpacing(0);
    columns->addWidget(m_categories);
    columns->addWidget(right, 1);

    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(6, 6, 6, 6);
    layout->setSpacing(4);
    auto* title = new QLabel(tr("Library"), this);
    title->setObjectName(QStringLiteral("panelTitle"));
    layout->addWidget(title);
    layout->addWidget(m_search);
    layout->addLayout(columns, 1);

    connect(m_categories, &QListWidget::currentRowChanged, this, [this] { onCategoryChanged(); });
    connect(m_search, &QLineEdit::textChanged, this, [this](const QString& text) {
        m_proxy->setFilterFixedString(text);
        m_tree->expandToDepth(text.isEmpty() ? 0 : 3);
        updateEmptyState();
    });
    connect(m_tree, &QTreeView::activated, this, [this](const QModelIndex& index) {
        const QString path = m_model->filePath(m_proxy->mapToSource(index));
        if (QFileInfo(path).isFile()) {
            emit fileActivated(path);
        }
    });
    connect(m_model, &QFileSystemModel::directoryLoaded, this, [this] { updateEmptyState(); });

    ensureLibraryLayout();
    setTheme(theme);
    m_categories->setCurrentRow(0);
}

void BrowserPanel::setTheme(const Theme* theme)
{
    m_theme = theme;
    const int w = m_theme->metricInt(QStringLiteral("browser.width"), 230);
    m_categories->setFixedWidth(qMax(80, w * 2 / 5));
    const int pad = m_theme->metricInt(QStringLiteral("panel.padding"), 8);
    layout()->setContentsMargins(pad, pad, pad, pad);
    QPalette pal = palette();
    pal.setColor(QPalette::Window, Qt::transparent);
    pal.setColor(QPalette::Base, Qt::transparent);
    pal.setColor(QPalette::Highlight, m_theme->color(QStringLiteral("browser.selection")));
    pal.setColor(QPalette::HighlightedText, m_theme->color(QStringLiteral("text.primary")));
    pal.setColor(QPalette::Text, m_theme->color(QStringLiteral("text.primary")));
    setPalette(pal);
    m_categories->setPalette(pal);
    m_tree->setPalette(pal);
    m_tree->viewport()->setPalette(pal);
    update();
}

QString BrowserPanel::libraryRoot()
{
    QSettings settings;
    const QString stored = settings.value(QLatin1String(kSettingsKey)).toString();
    if (!stored.isEmpty()) {
        return stored;
    }
    QString music = QStandardPaths::writableLocation(QStandardPaths::MusicLocation);
    if (music.isEmpty()) {
        music = QStandardPaths::writableLocation(QStandardPaths::HomeLocation);
    }
    return music + QStringLiteral("/Nylon Library");
}

void BrowserPanel::setLibraryRoot(const QString& path)
{
    QSettings settings;
    settings.setValue(QLatin1String(kSettingsKey), path);
}

QStringList BrowserPanel::categories()
{
    QStringList names;
    for (const Category& c : kCategories) {
        names.append(QString::fromLatin1(c.name));
    }
    return names;
}

QString BrowserPanel::folderForCategory(const QString& category)
{
    for (const Category& c : kCategories) {
        if (category == QLatin1String(c.name)) {
            return QString::fromLatin1(c.folder);
        }
    }
    return QString();
}

QString BrowserPanel::currentCategory() const
{
    const QListWidgetItem* item = m_categories->currentItem();
    return item ? item->text() : QString();
}

QString BrowserPanel::currentFolder() const
{
    const QString folder = folderForCategory(currentCategory());
    return folder.isEmpty() ? QString() : libraryRoot() + QLatin1Char('/') + folder;
}

bool BrowserPanel::isShowingEmptyState() const
{
    return m_empty->isVisibleTo(this);
}

int BrowserPanel::visibleEntryCount() const
{
    return m_proxy->rowCount(m_tree->rootIndex());
}

void BrowserPanel::selectCategory(const QString& category)
{
    const QList<QListWidgetItem*> items = m_categories->findItems(category, Qt::MatchExactly);
    if (!items.isEmpty()) {
        m_categories->setCurrentItem(items.first());
    }
}

void BrowserPanel::reload()
{
    ensureLibraryLayout();
    onCategoryChanged();
}

void BrowserPanel::ensureLibraryLayout()
{
    const QDir root(libraryRoot());
    for (const Category& c : kCategories) {
        root.mkpath(QString::fromLatin1(c.folder));
    }
}

void BrowserPanel::onCategoryChanged()
{
    const QString folder = currentFolder();
    if (folder.isEmpty()) {
        m_tree->setRootIndex(QModelIndex());
        updateEmptyState();
        return;
    }
    const QModelIndex source = m_model->setRootPath(folder);
    m_tree->setRootIndex(m_proxy->mapFromSource(source));
    // Show the location relative to the library so the label never carries
    // the user's home directory.
    m_info->setText(QDir(libraryRoot()).dirName() + QLatin1Char('/') + folderForCategory(currentCategory()));
    updateEmptyState();
    // The model populates asynchronously; check again once it has had a
    // chance to list the folder.
    QTimer::singleShot(50, this, [this] { updateEmptyState(); });
}

void BrowserPanel::updateEmptyState()
{
    const QString category = currentCategory();
    const bool empty = visibleEntryCount() == 0;
    if (empty) {
        if (m_search->text().isEmpty()) {
            m_empty->setText(tr("No %1 in the library yet.\nAdd files to the folder shown below.").arg(category));
        } else {
            m_empty->setText(tr("Nothing in %1 matches \"%2\".").arg(category, m_search->text()));
        }
    }
    m_empty->setVisible(empty);
    m_tree->setVisible(!empty);
}

void BrowserPanel::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    paint::panel(p, *m_theme, rect(), m_theme->color(QStringLiteral("browser.background")));
}

} // namespace nylon
