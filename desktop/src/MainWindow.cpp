#include "MainWindow.h"

#include "ArrangementView.h"
#include "ProjectBridge.h"
#include "SessionView.h"
#include "ThemeManager.h"
#include "TransportBar.h"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QFrame>
#include <QKeySequence>
#include <QMenu>
#include <QMenuBar>
#include <QStackedWidget>
#include <QStatusBar>
#include <QVBoxLayout>

namespace nylon {

MainWindow::MainWindow(ProjectBridge* bridge, ThemeManager* themes, QWidget* parent)
    : QMainWindow(parent)
    , m_bridge(bridge)
    , m_themes(themes)
    , m_transport(new TransportBar(bridge, this))
    , m_stack(new QStackedWidget(this))
    , m_session(new SessionView(bridge, &themes->theme(), this))
    , m_arrangement(new ArrangementView(bridge, &themes->theme(), this))
{
    setWindowTitle(QStringLiteral("Nylon"));
    resize(1280, 800);

    auto* central = new QWidget(this);
    central->setObjectName(QStringLiteral("central"));
    auto* layout = new QVBoxLayout(central);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    auto* line = new QFrame(central);
    line->setObjectName(QStringLiteral("transportSeparator"));
    line->setFrameShape(QFrame::NoFrame);
    line->setFixedHeight(1);
    line->setAutoFillBackground(true);
    layout->addWidget(m_transport);
    layout->addWidget(line);
    layout->addWidget(m_stack, 1);
    setCentralWidget(central);

    m_stack->addWidget(m_session);
    m_stack->addWidget(m_arrangement);
    m_stack->setCurrentWidget(m_session);

    statusBar()->setSizeGripEnabled(false);

    connect(m_transport, &TransportBar::message, this, &MainWindow::showStatus);
    connect(m_transport, &TransportBar::sessionRequested, this, &MainWindow::showSession);
    connect(m_transport, &TransportBar::arrangementRequested, this, &MainWindow::showArrangement);
    connect(m_themes, &ThemeManager::themeChanged, this, &MainWindow::applyTheme);
    connect(m_themes, &ThemeManager::loadFailed, this, [this](const QString& name, const QStringList& errors) {
        showStatus(tr("Theme '%1' not loaded: %2").arg(name, errors.join(QStringLiteral("; "))));
    });

    buildMenus();
    applyTheme(m_themes->theme());
}

bool MainWindow::isSessionVisible() const
{
    return m_stack->currentWidget() == m_session;
}

void MainWindow::showSession()
{
    m_stack->setCurrentWidget(m_session);
    m_transport->showSessionActive(true);
}

void MainWindow::showArrangement()
{
    m_stack->setCurrentWidget(m_arrangement);
    m_transport->showSessionActive(false);
}

void MainWindow::toggleView()
{
    if (isSessionVisible()) {
        showArrangement();
    } else {
        showSession();
    }
}

void MainWindow::buildMenus()
{
    QMenu* edit = menuBar()->addMenu(tr("&Edit"));
    QAction* undo = edit->addAction(tr("&Undo"), QKeySequence::Undo, this, [this] {
        if (!m_bridge->undo()) {
            showStatus(tr("Nothing to undo."));
        }
    });
    undo->setObjectName(QStringLiteral("actionUndo"));
    QAction* redo = edit->addAction(tr("&Redo"), QKeySequence::Redo, this, [this] {
        if (!m_bridge->redo()) {
            showStatus(tr("Nothing to redo."));
        }
    });
    redo->setObjectName(QStringLiteral("actionRedo"));

    QMenu* track = menuBar()->addMenu(tr("&Track"));
    QAction* add = track->addAction(tr("Add &Track"), QKeySequence(Qt::CTRL | Qt::Key_T), this, [this] {
        if (!m_bridge->addTrack()) {
            showStatus(tr("Could not add a track."));
        }
    });
    add->setObjectName(QStringLiteral("actionAddTrack"));

    QMenu* view = menuBar()->addMenu(tr("&View"));
    view->addAction(tr("&Session"), QKeySequence(Qt::Key_F1), this, &MainWindow::showSession);
    view->addAction(tr("&Arrangement"), QKeySequence(Qt::Key_F2), this, &MainWindow::showArrangement);
    view->addAction(tr("Toggle Session/Arrangement"), QKeySequence(Qt::Key_Tab), this, &MainWindow::toggleView);
    view->addSeparator();

    QMenu* themeMenu = view->addMenu(tr("&Theme"));
    m_themeActions = new QActionGroup(this);
    m_themeActions->setExclusive(true);
    const QStringList names = ThemeManager::builtinNames();
    for (const QString& name : names) {
        QString label = name;
        if (!label.isEmpty()) {
            label[0] = label[0].toUpper();
        }
        QAction* a = themeMenu->addAction(label);
        a->setCheckable(true);
        a->setData(name);
        a->setChecked(name == m_themes->currentName());
        m_themeActions->addAction(a);
        connect(a, &QAction::triggered, this, [this, name] { m_themes->load(name); });
    }
    themeMenu->addSeparator();
    themeMenu->addAction(tr("&Reload Theme"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_R), this, [this] {
        if (m_themes->reload()) {
            showStatus(tr("Theme reloaded."));
        }
    });
}

void MainWindow::applyTheme(const Theme& theme)
{
    qApp->setStyleSheet(theme.styleSheet());
    m_transport->applyTheme(theme);
    m_session->setTheme(&theme);
    m_arrangement->setTheme(&theme);
    if (auto* line = findChild<QFrame*>(QStringLiteral("transportSeparator"))) {
        QPalette pal = line->palette();
        pal.setColor(QPalette::Window, theme.color(QStringLiteral("separator")));
        line->setPalette(pal);
        line->setFixedHeight(qMax(1, theme.metricInt(QStringLiteral("separator"), 1)));
    }
    if (m_themeActions) {
        for (QAction* a : m_themeActions->actions()) {
            a->setChecked(a->data().toString() == m_themes->currentName());
        }
    }
}

void MainWindow::showStatus(const QString& text)
{
    statusBar()->showMessage(text, 4000);
}

} // namespace nylon
