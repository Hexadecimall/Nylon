#include "StartScreen.h"

#include "BrowserPanel.h"
#include "Theme.h"
#include "widgets/FlatButton.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QSettings>
#include <QVBoxLayout>

namespace nylon {

namespace {
const char* const kRecentKey = "projects/recent";
constexpr int kMaxRecent = 8;
} // namespace

StartScreen::StartScreen(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_card(new QWidget(this))
    , m_sidebar(new QWidget(m_card))
    , m_recentPane(new QWidget(m_card))
    , m_title(new QLabel(tr("Projects"), m_card))
    , m_version(new QLabel(m_card))
    , m_tagline(new QLabel(tr("START"), m_sidebar))
    , m_new(new FlatButton(theme, m_sidebar))
    , m_recording(new FlatButton(theme, m_sidebar))
    , m_production(new FlatButton(theme, m_sidebar))
    , m_open(new FlatButton(theme, m_sidebar))
    , m_recentTitle(new QLabel(tr("RECENT PROJECTS"), m_recentPane))
    , m_recent(new QListWidget(m_recentPane))
    , m_recentEmpty(new QLabel(m_recentPane))
    , m_shortcuts(new QLabel(m_recentPane))
    , m_footer(new QLabel(m_sidebar))
{
    setObjectName(QStringLiteral("start"));
    m_card->setObjectName(QStringLiteral("startCard"));
    m_sidebar->setObjectName(QStringLiteral("startSidebar"));
    m_recentPane->setObjectName(QStringLiteral("startRecent"));
    m_sidebar->setFixedWidth(220);

    m_title->setObjectName(QStringLiteral("startTitle"));
    m_version->setObjectName(QStringLiteral("secondary"));
    m_version->setText(tr("Nylon %1").arg(QCoreApplication::applicationVersion()));
    m_tagline->setObjectName(QStringLiteral("secondary"));

    m_new->setText(tr("Empty Project"));
    m_new->setProminent(true);
    m_new->setStatusTip(tr("Start an empty project (Ctrl+N)."));
    m_new->setActiveColorKey(QStringLiteral("accent"));
    m_recording->setText(tr("Recording Setup"));
    m_recording->setStatusTip(tr("Start with four audio tracks."));
    m_production->setText(tr("Production Setup"));
    m_production->setStatusTip(tr("Start with audio and MIDI tracks."));
    m_open->setText(tr("Open Project"));
    m_open->setStatusTip(tr("Open a project bundle (Ctrl+O)."));

    m_recentTitle->setObjectName(QStringLiteral("secondary"));
    m_recent->setObjectName(QStringLiteral("recentProjects"));
    m_recent->setFrameShape(QFrame::NoFrame);
    m_recent->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_recent->setStatusTip(tr("Projects opened recently. Double-click to open."));
    m_recentEmpty->setObjectName(QStringLiteral("secondary"));
    m_recentEmpty->setAlignment(Qt::AlignCenter);
    m_recentEmpty->setWordWrap(true);
    m_shortcuts->setObjectName(QStringLiteral("startShortcuts"));
    m_shortcuts->setText(tr("CREATE AUDIO TRACK     Ctrl+T\n"
                            "CREATE MIDI TRACK      Ctrl+Shift+T\n"
                            "COMMAND PALETTE        Ctrl+K"));
    m_shortcuts->setAlignment(Qt::AlignLeft | Qt::AlignVCenter);
    m_footer->setObjectName(QStringLiteral("secondary"));
    m_footer->setWordWrap(true);

    auto* heading = new QHBoxLayout;
    heading->setSpacing(12);
    heading->addWidget(m_title);
    heading->addStretch(1);
    heading->addWidget(m_version, 0, Qt::AlignVCenter);

    auto* sidebarLayout = new QVBoxLayout(m_sidebar);
    sidebarLayout->setSpacing(8);
    sidebarLayout->addWidget(m_tagline);
    sidebarLayout->addSpacing(4);
    sidebarLayout->addWidget(m_new);
    sidebarLayout->addWidget(m_recording);
    sidebarLayout->addWidget(m_production);
    sidebarLayout->addSpacing(14);
    sidebarLayout->addWidget(m_open);
    sidebarLayout->addStretch(1);
    sidebarLayout->addWidget(m_footer);

    auto* recentLayout = new QVBoxLayout(m_recentPane);
    recentLayout->setSpacing(8);
    recentLayout->addWidget(m_recentTitle);
    recentLayout->addWidget(m_recent, 1);
    recentLayout->addWidget(m_recentEmpty, 1);
    recentLayout->addWidget(m_shortcuts);

    auto* body = new QHBoxLayout;
    body->setSpacing(10);
    body->addWidget(m_sidebar);
    body->addWidget(m_recentPane, 1);

    auto* cardLayout = new QVBoxLayout(m_card);
    cardLayout->setSpacing(14);
    cardLayout->addLayout(heading);
    cardLayout->addLayout(body, 1);

    auto* root = new QVBoxLayout(this);
    root->setContentsMargins(0, 0, 0, 0);
    root->addWidget(m_card);

    connect(m_new, &FlatButton::clicked, this, &StartScreen::newProjectRequested);
    connect(m_recording, &FlatButton::clicked, this, [this] { emit templateRequested(4, 0); });
    connect(m_production, &FlatButton::clicked, this, [this] { emit templateRequested(1, 3); });
    connect(m_open, &FlatButton::clicked, this, &StartScreen::openProjectRequested);
    connect(m_recent, &QListWidget::itemActivated, this, [this](QListWidgetItem* item) {
        emit recentProjectRequested(item->data(Qt::UserRole).toString());
    });

    setPersistenceAvailable(true);
    setTheme(theme);
    reloadRecent();
}

void StartScreen::setTheme(const Theme* theme)
{
    m_theme = theme;
    m_new->setTheme(theme);
    m_recording->setTheme(theme);
    m_production->setTheme(theme);
    m_open->setTheme(theme);
    const int fontPx = theme->metricInt(QStringLiteral("font.size"), 11);
    QFont big = font();
    big.setPixelSize(fontPx * 2);
    big.setWeight(QFont::DemiBold);
    m_title->setFont(big);
    const int h = theme->metricInt(QStringLiteral("control.height"), 20);
    m_new->setFixedHeight(h + 6);
    m_recording->setFixedHeight(h + 6);
    m_production->setFixedHeight(h + 6);
    m_open->setFixedHeight(h + 6);
    const int pad = theme->metricInt(QStringLiteral("panel.padding"), 8);
    m_card->layout()->setContentsMargins(pad * 3, pad * 2, pad * 3, pad * 3);
    m_sidebar->layout()->setContentsMargins(pad * 2, pad * 2, pad * 2, pad * 2);
    m_recentPane->layout()->setContentsMargins(pad * 2, pad * 2, pad * 2, pad * 2);
    QPalette pal = m_recent->palette();
    pal.setColor(QPalette::Base, Qt::transparent);
    pal.setColor(QPalette::Highlight, theme->color(QStringLiteral("browser.selection")));
    pal.setColor(QPalette::HighlightedText, theme->color(QStringLiteral("text.primary")));
    m_recent->setPalette(pal);
    m_footer->setText(tr("LIBRARY\n%1").arg(QDir(BrowserPanel::libraryRoot()).dirName()));
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
    m_shortcuts->setVisible(empty);
    m_recentEmpty->setText(m_persistence ? tr("No recent projects\nCreate a project or open an existing bundle.")
                                         : tr("Recent projects appear here once projects can be saved."));
}

void StartScreen::paintEvent(QPaintEvent*)
{
}

} // namespace nylon
