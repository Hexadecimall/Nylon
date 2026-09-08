#include "StartScreen.h"

#include "BrowserPanel.h"
#include "PanelPaint.h"
#include "Theme.h"
#include "widgets/FlatButton.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QGridLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QPainter>
#include <QSettings>
#include <QVBoxLayout>

namespace nylon {

namespace {
const char* const kRecentKey = "projects/recent";
constexpr int kMaxRecent = 8;
constexpr int kCardWidth = 640;
} // namespace

StartScreen::StartScreen(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_card(new QWidget(this))
    , m_title(new QLabel(QStringLiteral("Nylon"), m_card))
    , m_version(new QLabel(m_card))
    , m_tagline(new QLabel(tr("Session and arrangement workstation"), m_card))
    , m_new(new FlatButton(theme, m_card))
    , m_open(new FlatButton(theme, m_card))
    , m_recentTitle(new QLabel(tr("Recent projects"), m_card))
    , m_recent(new QListWidget(m_card))
    , m_recentEmpty(new QLabel(m_card))
    , m_footer(new QLabel(m_card))
{
    setObjectName(QStringLiteral("start"));
    m_card->setObjectName(QStringLiteral("startCard"));
    m_card->setFixedWidth(kCardWidth);

    m_title->setObjectName(QStringLiteral("startTitle"));
    m_version->setObjectName(QStringLiteral("secondary"));
    m_version->setText(QCoreApplication::applicationVersion());
    m_tagline->setObjectName(QStringLiteral("secondary"));

    m_new->setText(tr("New Project"));
    m_new->setStatusTip(tr("Start an empty project (Ctrl+N)."));
    m_new->setActiveColorKey(QStringLiteral("accent"));
    m_open->setText(tr("Open Project..."));
    m_open->setStatusTip(tr("Open a project bundle (Ctrl+O)."));

    m_recentTitle->setObjectName(QStringLiteral("secondary"));
    m_recent->setObjectName(QStringLiteral("recentProjects"));
    m_recent->setFrameShape(QFrame::NoFrame);
    m_recent->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_recent->setStatusTip(tr("Projects opened recently. Double-click to open."));
    m_recentEmpty->setObjectName(QStringLiteral("secondary"));
    m_recentEmpty->setWordWrap(true);
    m_footer->setObjectName(QStringLiteral("secondary"));
    m_footer->setWordWrap(true);

    auto* heading = new QHBoxLayout;
    heading->setSpacing(10);
    heading->addWidget(m_title);
    heading->addWidget(m_version, 0, Qt::AlignBottom);
    heading->addStretch(1);

    auto* actions = new QHBoxLayout;
    actions->setSpacing(8);
    actions->addWidget(m_new);
    actions->addWidget(m_open);
    actions->addStretch(1);

    auto* layout = new QVBoxLayout(m_card);
    layout->setSpacing(6);
    layout->addLayout(heading);
    layout->addWidget(m_tagline);
    layout->addSpacing(10);
    layout->addLayout(actions);
    layout->addSpacing(14);
    layout->addWidget(m_recentTitle);
    layout->addWidget(m_recent);
    layout->addWidget(m_recentEmpty);
    layout->addSpacing(6);
    layout->addWidget(m_footer);

    // Center the card; stretches absorb the remaining canvas.
    auto* grid = new QGridLayout(this);
    grid->setContentsMargins(0, 0, 0, 0);
    grid->setRowStretch(0, 2);
    grid->setRowStretch(2, 3);
    grid->setColumnStretch(0, 1);
    grid->setColumnStretch(2, 1);
    grid->addWidget(m_card, 1, 1, Qt::AlignCenter);

    connect(m_new, &FlatButton::clicked, this, &StartScreen::newProjectRequested);
    connect(m_open, &FlatButton::clicked, this, &StartScreen::openProjectRequested);
    connect(m_recent, &QListWidget::itemActivated, this, [this](QListWidgetItem* item) {
        emit recentProjectRequested(item->data(Qt::UserRole).toString());
    });

    setPersistenceAvailable(false);
    setTheme(theme);
    reloadRecent();
}

void StartScreen::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_new->setTheme(theme);
    m_open->setTheme(theme);
    const int fontPx = theme->metricInt(QStringLiteral("font.size"), 11);
    QFont big = font();
    big.setPixelSize(fontPx * 3);
    big.setWeight(QFont::DemiBold);
    m_title->setFont(big);
    const int h = theme->metricInt(QStringLiteral("control.height"), 20);
    m_new->setFixedHeight(h + 10);
    m_open->setFixedHeight(h + 10);
    m_new->setMinimumWidth(140);
    m_open->setMinimumWidth(140);
    m_recent->setFixedHeight(qMax(1, m_recent->count()) * (fontPx * 2 + 4) + 4);
    const int pad = theme->metricInt(QStringLiteral("panel.padding"), 8) * 3;
    m_card->layout()->setContentsMargins(pad, pad, pad, pad);
    QPalette pal = m_recent->palette();
    pal.setColor(QPalette::Base, Qt::transparent);
    pal.setColor(QPalette::Highlight, theme->color(QStringLiteral("browser.selection")));
    pal.setColor(QPalette::HighlightedText, theme->color(QStringLiteral("text.primary")));
    m_recent->setPalette(pal);
    m_footer->setText(tr("Library: %1").arg(QDir(BrowserPanel::libraryRoot()).dirName()));
    update();
}

void StartScreen::setPersistenceAvailable(bool available)
{
    m_persistence = available;
    m_open->setEnabled(available);
    m_open->setToolTip(available ? QString() : tr("Opening projects is not available until the core can read project bundles."));
    m_recent->setEnabled(available);
    reloadRecent();
}

QRect StartScreen::cardRect() const
{
    return m_card->geometry();
}

QStringList StartScreen::recentProjects()
{
    QSettings settings;
    QStringList list = settings.value(QLatin1String(kRecentKey)).toStringList();
    list.erase(std::remove_if(list.begin(), list.end(), [](const QString& p) { return !QFileInfo::exists(p); }), list.end());
    return list;
}

void StartScreen::addRecentProject(const QString& path)
{
    QSettings settings;
    QStringList list = settings.value(QLatin1String(kRecentKey)).toStringList();
    list.removeAll(path);
    list.prepend(path);
    while (list.size() > kMaxRecent) {
        list.removeLast();
    }
    settings.setValue(QLatin1String(kRecentKey), list);
}

void StartScreen::clearRecentProjects()
{
    QSettings settings;
    settings.remove(QLatin1String(kRecentKey));
}

void StartScreen::reloadRecent()
{
    m_recent->clear();
    const QStringList list = recentProjects();
    for (const QString& path : list) {
        auto* item = new QListWidgetItem(QFileInfo(path).completeBaseName(), m_recent);
        item->setData(Qt::UserRole, path);
        item->setToolTip(path);
    }
    const bool empty = list.isEmpty();
    m_recent->setVisible(!empty);
    m_recentEmpty->setVisible(empty);
    m_recentEmpty->setText(m_persistence ? tr("Nothing yet. Projects you open or save show up here.")
                                         : tr("Recent projects appear here once projects can be saved."));
    const int fontPx = m_theme->metricInt(QStringLiteral("font.size"), 11);
    m_recent->setFixedHeight(qMax(1, m_recent->count()) * (fontPx * 2 + 4) + 4);
    m_card->adjustSize();
}

void StartScreen::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.fillRect(rect(), Qt::transparent);
    paint::panel(p, *m_theme, m_card->geometry().adjusted(-1, -1, 1, 1), m_theme->color(QStringLiteral("panel")));
}

} // namespace nylon
