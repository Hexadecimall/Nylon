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

#include <iterator>

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
    , m_places(new QListWidget(this))
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
    m_categories->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    // Row heights come from the theme and change with it, which a cached
    // uniform size would ignore.
    m_categories->setUniformItemSizes(false);
    m_categories->setTextElideMode(Qt::ElideRight);
    m_categories->setStatusTip(tr("Library categories. Each one is a folder in the library."));
    for (const Category& c : kCategories) {
        m_categories->addItem(QString::fromLatin1(c.name));
    }

    m_places->setObjectName(QStringLiteral("browserPlaces"));
    m_places->setFrameShape(QFrame::NoFrame);
    m_places->setSelectionMode(QAbstractItemView::SingleSelection);
    m_places->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_places->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_places->setUniformItemSizes(false);
    m_places->setTextElideMode(Qt::ElideRight);
    m_places->setStatusTip(tr("Folders outside the library."));
    for (const auto& place : places()) {
        auto* item = new QListWidgetItem(place.first, m_places);
        item->setData(Qt::UserRole, place.second);
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
    m_empty->setAlignment(Qt::AlignLeft | Qt::AlignTop);
    m_empty->setWordWrap(true);
    m_empty->setMargin(4);

    m_info->setObjectName(QStringLiteral("secondary"));
    m_info->setMargin(4);

    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(6, 6, 6, 6);
    layout->setSpacing(4);
    auto* title = new QLabel(tr("LIBRARY"), this);
    title->setObjectName(QStringLiteral("panelTitle"));
    auto* categoryTitle = new QLabel(tr("CATEGORIES"), this);
    categoryTitle->setObjectName(QStringLiteral("sectionLabel"));
    auto* placeTitle = new QLabel(tr("PLACES"), this);
    placeTitle->setObjectName(QStringLiteral("sectionLabel"));
    auto* contentTitle = new QLabel(tr("FILES"), this);
    contentTitle->setObjectName(QStringLiteral("sectionLabel"));
    layout->addWidget(title);
    layout->addWidget(m_search);
    layout->addWidget(categoryTitle);
    layout->addWidget(m_categories);
    layout->addWidget(placeTitle);
    layout->addWidget(m_places);
    layout->addWidget(contentTitle);
    layout->addWidget(m_empty);
    layout->addWidget(m_tree, 1);
    layout->addWidget(m_info);

    connect(m_categories, &QListWidget::currentRowChanged, this, [this] { onCategoryChanged(); });
    connect(m_places, &QListWidget::currentRowChanged, this, [this] { onPlaceChanged(); });
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
    const int browserWidth = m_theme->metricInt(QStringLiteral("browser.width"), 230);
    const int rowHeight = qMax(22, m_theme->metricInt(QStringLiteral("control.height"), 24));
    const int categoryCount = static_cast<int>(std::size(kCategories));
    const int pad = m_theme->metricInt(QStringLiteral("panel.padding"), 8);
    for (int index = 0; index < categoryCount; ++index) {
        m_categories->item(index)->setSizeHint(QSize(qMax(64, browserWidth - pad * 2), rowHeight));
    }
    // The list is exactly as tall as its rows, so the section below it
    // starts right after the last one.
    m_categories->setFixedHeight(categoryCount * rowHeight + 2);
    const int placeCount = m_places->count();
    for (int index = 0; index < placeCount; ++index) {
        m_places->item(index)->setSizeHint(QSize(qMax(64, browserWidth - pad * 2), rowHeight));
    }
    m_places->setFixedHeight(placeCount * rowHeight + 2);
    layout()->setContentsMargins(pad, pad, pad, pad);
    QPalette pal = palette();
    pal.setColor(QPalette::Window, Qt::transparent);
    pal.setColor(QPalette::Base, Qt::transparent);
    pal.setColor(QPalette::Highlight, m_theme->color(QStringLiteral("browser.selection")));
    pal.setColor(QPalette::HighlightedText, m_theme->color(QStringLiteral("text.primary")));
    pal.setColor(QPalette::Text, m_theme->color(QStringLiteral("text.primary")));
    setPalette(pal);
    m_categories->setPalette(pal);
    m_places->setPalette(pal);
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

QList<QPair<QString, QString>> BrowserPanel::places()
{
    // The standard folders a person keeps material in. A location the
    // platform does not define, or that does not exist, is left out.
    const QList<QPair<QString, QStandardPaths::StandardLocation>> wanted {
        {tr("Home"), QStandardPaths::HomeLocation},
        {tr("Music"), QStandardPaths::MusicLocation},
        {tr("Downloads"), QStandardPaths::DownloadLocation},
        {tr("Desktop"), QStandardPaths::DesktopLocation},
        {tr("Documents"), QStandardPaths::DocumentsLocation},
    };
    QList<QPair<QString, QString>> found;
    for (const auto& entry : wanted) {
        const QString path = QStandardPaths::writableLocation(entry.second);
        if (!path.isEmpty() && QDir(path).exists()) {
            found.append({entry.first, path});
        }
    }
    return found;
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

void BrowserPanel::showFolder(const QString& path, const QString& label)
{
    if (path.isEmpty()) {
        m_tree->setRootIndex(QModelIndex());
        updateEmptyState();
        return;
    }
    const QModelIndex source = m_model->setRootPath(path);
    m_tree->setRootIndex(m_proxy->mapFromSource(source));
    m_info->setText(label);
    updateEmptyState();
    // The model populates asynchronously; check again once it has had a
    // chance to list the folder.
    QTimer::singleShot(50, this, [this] { updateEmptyState(); });
}

void BrowserPanel::onCategoryChanged()
{
    if (m_categories->currentRow() >= 0) {
        const QSignalBlocker block(m_places);
        m_places->clearSelection();
        m_places->setCurrentRow(-1);
    }
    // Show the location relative to the library so the label never carries
    // the user's home directory.
    showFolder(currentFolder(),
        QDir(libraryRoot()).dirName() + QLatin1Char('/') + folderForCategory(currentCategory()));
}

void BrowserPanel::onPlaceChanged()
{
    const QListWidgetItem* item = m_places->currentItem();
    if (!item) {
        return;
    }
    {
        const QSignalBlocker block(m_categories);
        m_categories->clearSelection();
        m_categories->setCurrentRow(-1);
    }
    showFolder(item->data(Qt::UserRole).toString(), item->text());
}

void BrowserPanel::updateEmptyState()
{
    const QString category = currentCategory();
    const bool empty = visibleEntryCount() == 0;
    if (empty) {
        if (!m_search->text().isEmpty()) {
            m_empty->setText(tr("Nothing here matches \"%1\".").arg(m_search->text()));
        } else if (category.isEmpty()) {
            m_empty->setText(tr("This folder is empty."));
        } else {
            m_empty->setText(tr("No %1 yet. Drop files in the folder below.").arg(category.toLower()));
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
