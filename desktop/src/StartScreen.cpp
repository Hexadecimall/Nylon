#include "StartScreen.h"

#include "PanelPaint.h"
#include "Theme.h"
#include "widgets/FlatButton.h"

#include <QCoreApplication>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QPainter>
#include <QSettings>
#include <QVBoxLayout>

namespace nylon {

namespace {
const char* const kRecentKey = "projects/recent";
constexpr int kMaxRecent = 10;
} // namespace

StartScreen::StartScreen(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_title(new QLabel(QStringLiteral("Nylon"), this))
    , m_version(new QLabel(this))
    , m_new(new FlatButton(theme, this))
    , m_open(new FlatButton(theme, this))
    , m_recentTitle(new QLabel(tr("Recent"), this))
    , m_recent(new QListWidget(this))
    , m_recentEmpty(new QLabel(this))
{
    setObjectName(QStringLiteral("start"));
    m_title->setObjectName(QStringLiteral("startTitle"));
    m_version->setObjectName(QStringLiteral("secondary"));
    m_version->setText(tr("Version %1").arg(QCoreApplication::applicationVersion()));

    m_new->setText(tr("New Project"));
    m_new->setStatusTip(tr("Start an empty project (Ctrl+N)."));
    m_open->setText(tr("Open..."));
    m_open->setStatusTip(tr("Open a project bundle (Ctrl+O)."));

    m_recentTitle->setObjectName(QStringLiteral("secondary"));
    m_recent->setObjectName(QStringLiteral("recentProjects"));
    m_recent->setFrameShape(QFrame::NoFrame);
    m_recent->setStatusTip(tr("Projects opened recently. Double-click to open."));
    m_recentEmpty->setObjectName(QStringLiteral("secondary"));
    m_recentEmpty->setWordWrap(true);

    auto* buttons = new QHBoxLayout;
    buttons->setSpacing(6);
    buttons->addWidget(m_new);
    buttons->addWidget(m_open);
    buttons->addStretch(1);

    auto* column = new QVBoxLayout;
    column->setSpacing(10);
    column->addStretch(2);
    column->addWidget(m_title);
    column->addWidget(m_version);
    column->addSpacing(16);
    column->addLayout(buttons);
    column->addSpacing(24);
    column->addWidget(m_recentTitle);
    column->addWidget(m_recent, 1);
    column->addWidget(m_recentEmpty);
    column->addStretch(3);

    auto* layout = new QHBoxLayout(this);
    layout->addStretch(1);
    layout->addLayout(column, 2);
    layout->addStretch(1);

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
    QFont big = font();
    big.setPixelSize(theme->metricInt(QStringLiteral("font.size"), 11) * 3);
    big.setWeight(QFont::DemiBold);
    m_title->setFont(big);
    const int h = theme->metricInt(QStringLiteral("control.height"), 20);
    m_new->setFixedHeight(h + 8);
    m_open->setFixedHeight(h + 8);
    m_recent->setMaximumHeight(h * 8);
    QPalette pal = m_recent->palette();
    pal.setColor(QPalette::Base, theme->color(QStringLiteral("browser.background")));
    pal.setColor(QPalette::Highlight, theme->color(QStringLiteral("browser.selection")));
    pal.setColor(QPalette::HighlightedText, theme->color(QStringLiteral("text.primary")));
    m_recent->setPalette(pal);
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
    m_recentEmpty->setText(m_persistence ? tr("No recent projects.") : tr("Recent projects appear here once projects can be saved."));
}

void StartScreen::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const int pad = m_theme->metricInt(QStringLiteral("panel.gap"), 6);
    paint::panel(p, *m_theme, rect().adjusted(pad, pad, -pad, -pad), m_theme->color(QStringLiteral("panel")));
}

} // namespace nylon
