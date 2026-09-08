#include "MainWindow.h"

#include "ArrangementView.h"
#include "BrowserPanel.h"
#include "CommandPalette.h"
#include "Shortcuts.h"
#include "DetailPanel.h"
#include "MixerSection.h"
#include "PreferencesDialog.h"
#include "ProjectBridge.h"
#include "SessionView.h"
#include "StartScreen.h"
#include "ThemeManager.h"
#include "TitleBar.h"
#include "TransportBar.h"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QCloseEvent>
#include <QFileDialog>
#include <QFileInfo>
#include <QInputDialog>
#include <QStandardPaths>
#include <QKeySequence>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QScrollBar>
#include <QSettings>
#include <QSplitter>
#include <QStackedWidget>
#include <QStatusBar>
#include <QVBoxLayout>
#include <QWindow>
#include <QPainter>
#include <QPainterPath>
#include <QMouseEvent>

namespace nylon {

MainWindow::MainWindow(ProjectBridge* bridge, ThemeManager* themes, QWidget* parent)
    : QMainWindow(parent)
    , m_bridge(bridge)
    , m_themes(themes)
    , m_root(new QStackedWidget(this))
    , m_start(new StartScreen(&themes->theme(), this))
{
    setWindowTitle(QStringLiteral("Nylon"));
    resize(m_workspaceSize);
    // Frameless with a painted rounded outline; the title bar below carries
    // the menus and window controls on every platform.
    setWindowFlags(Qt::Window | Qt::FramelessWindowHint);
    setAttribute(Qt::WA_TranslucentBackground);
    setMouseTracking(true);
    // QMainWindow::menuBar() is never used: setMenuWidget would delete it
    // and a later call would replace the title bar with a plain bar.
    m_titleBar = new TitleBar(&themes->theme(), this);
    setMenuWidget(m_titleBar);
    connect(m_titleBar, &TitleBar::closeRequested, this, &QWidget::close);
    connect(m_titleBar, &TitleBar::minimizeRequested, this, &QWidget::showMinimized);
    connect(m_titleBar, &TitleBar::zoomRequested, this, &MainWindow::toggleMaximized);
    connect(this, &QWidget::windowTitleChanged, m_titleBar, &TitleBar::setTitle);
    m_titleBar->setTitle(windowTitle());

    buildWorkspace();
    m_root->addWidget(m_start);
    m_root->addWidget(m_workspace);
    // The hidden workstation must not force the compact launcher to its
    // larger minimum size.
    m_root->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Ignored);
    setCentralWidget(m_root);
    statusBar()->setSizeGripEnabled(false);
    statusBar()->hide();
    connect(statusBar(), &QStatusBar::messageChanged, this, [this](const QString& text) {
        statusBar()->setVisible(!text.isEmpty());
    });
    // Edge resizing works on every child, so the filter sits on the app.
    qApp->installEventFilter(this);

    connect(m_start, &StartScreen::newProjectRequested, this, &MainWindow::newProject);
    connect(m_start, &StartScreen::openProjectRequested, this, &MainWindow::openProject);
    connect(m_start, &StartScreen::recentProjectRequested, this, &MainWindow::openProjectAt);
    connect(m_themes, &ThemeManager::themeChanged, this, &MainWindow::applyTheme);
    connect(m_themes, &ThemeManager::loadFailed, this, [this](const QString& name, const QStringList& errors) {
        showStatus(tr("Theme '%1' not loaded: %2").arg(name, errors.join(QStringLiteral("; "))));
    });

    buildMenus();
    Shortcuts::apply(namedActions());
    applyTheme(m_themes->theme());
    restoreLayout();
    m_workspaceSize = QSize(qMax(width(), 1280), qMax(height(), 800));
    updateEditActions();
    showStartScreen();
}

MainWindow::~MainWindow()
{
    qApp->removeEventFilter(this);
}

void MainWindow::toggleMaximized()
{
    if (isMaximized() || isFullScreen()) {
        showNormal();
    } else {
        showMaximized();
    }
}

Qt::Edges MainWindow::edgesAt(const QPoint& pos) const
{
    if (isMaximized() || isFullScreen()) {
        return Qt::Edges();
    }
    const int grip = 6;
    Qt::Edges edges;
    if (pos.x() <= grip) {
        edges |= Qt::LeftEdge;
    }
    if (pos.x() >= width() - grip) {
        edges |= Qt::RightEdge;
    }
    if (pos.y() <= grip) {
        edges |= Qt::TopEdge;
    }
    if (pos.y() >= height() - grip) {
        edges |= Qt::BottomEdge;
    }
    return edges;
}

void MainWindow::updateResizeCursor(const QPoint& pos)
{
    const Qt::Edges e = edgesAt(pos);
    Qt::CursorShape shape = Qt::ArrowCursor;
    if ((e & Qt::LeftEdge && e & Qt::TopEdge) || (e & Qt::RightEdge && e & Qt::BottomEdge)) {
        shape = Qt::SizeFDiagCursor;
    } else if ((e & Qt::RightEdge && e & Qt::TopEdge) || (e & Qt::LeftEdge && e & Qt::BottomEdge)) {
        shape = Qt::SizeBDiagCursor;
    } else if (e & (Qt::LeftEdge | Qt::RightEdge)) {
        shape = Qt::SizeHorCursor;
    } else if (e & (Qt::TopEdge | Qt::BottomEdge)) {
        shape = Qt::SizeVerCursor;
    }
    if (shape == Qt::ArrowCursor) {
        unsetCursor();
    } else {
        setCursor(shape);
    }
}

bool MainWindow::eventFilter(QObject* watched, QEvent* event)
{
    if (event->type() == QEvent::MouseButtonPress || event->type() == QEvent::MouseMove) {
        auto* w = qobject_cast<QWidget*>(watched);
        if (w && w->window() == this) {
            auto* me = static_cast<QMouseEvent*>(event);
            const QPoint pos = mapFromGlobal(me->globalPosition().toPoint());
            const Qt::Edges edges = edgesAt(pos);
            if (event->type() == QEvent::MouseMove && !(me->buttons() & Qt::LeftButton)) {
                updateResizeCursor(pos);
            } else if (event->type() == QEvent::MouseButtonPress && edges && me->button() == Qt::LeftButton) {
                if (QWindow* handle = windowHandle()) {
                    if (handle->startSystemResize(edges)) return true;
                }
            }
        }
    }
    return QMainWindow::eventFilter(watched, event);
}

void MainWindow::mousePressEvent(QMouseEvent* event)
{
    const Qt::Edges edges = edgesAt(event->pos());
    if (edges && event->button() == Qt::LeftButton) {
        if (QWindow* handle = windowHandle()) {
            if (handle->startSystemResize(edges)) return;
        }
    }
    QMainWindow::mousePressEvent(event);
}

void MainWindow::mouseMoveEvent(QMouseEvent* event)
{
    updateResizeCursor(event->pos());
    QMainWindow::mouseMoveEvent(event);
}

void MainWindow::paintEvent(QPaintEvent*)
{
    const Theme& theme = m_themes->theme();
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing, true);
    const int radius = (isMaximized() || isFullScreen()) ? 0 : qBound(0, theme.metricInt(QStringLiteral("radius"), 8), 32);
    const int border = qBound(0, theme.metricInt(QStringLiteral("window.border"), 1), 4);
    QPainterPath path;
    path.addRoundedRect(QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5), radius, radius);
    p.fillPath(path, theme.color(QStringLiteral("background")));
    // Title band across the top, clipped to the rounded outline.
    p.save();
    p.setClipPath(path);
    p.fillRect(QRect(0, 0, width(), m_titleBar->height()), theme.color(QStringLiteral("titlebar.background")));
    p.restore();
    if (border > 0) {
        p.setPen(QPen(theme.color(QStringLiteral("window.border")), border));
        p.setBrush(Qt::NoBrush);
        p.drawPath(path);
    }
}

void MainWindow::resizeEvent(QResizeEvent* event)
{
    QMainWindow::resizeEvent(event);
    update();
}

void MainWindow::buildWorkspace()
{
    const Theme* theme = &m_themes->theme();
    m_workspace = new QWidget(this);
    m_workspace->setObjectName(QStringLiteral("workspace"));
    m_transport = new TransportBar(m_bridge, theme, m_workspace);
    m_browser = new BrowserPanel(theme, m_workspace);
    m_session = new SessionView(m_bridge, theme, m_workspace);
    m_mixer = new MixerSection(m_bridge, theme, m_workspace);
    m_arrangement = new ArrangementView(m_bridge, theme, m_workspace);
    m_detail = new DetailPanel(m_bridge, theme, m_workspace);

    m_mixer->followScrollBar(m_session->horizontalScrollBar());

    m_views = new QStackedWidget(m_workspace);
    m_views->addWidget(m_session);
    m_views->addWidget(m_arrangement);

    m_vertical = new QSplitter(Qt::Vertical, m_workspace);
    m_vertical->setObjectName(QStringLiteral("verticalSplit"));
    m_vertical->setChildrenCollapsible(false);
    m_vertical->addWidget(m_views);
    m_vertical->addWidget(m_mixer);
    m_vertical->setStretchFactor(0, 1);
    m_vertical->setStretchFactor(1, 0);

    m_horizontal = new QSplitter(Qt::Horizontal, m_workspace);
    m_horizontal->setObjectName(QStringLiteral("horizontalSplit"));
    m_horizontal->setChildrenCollapsible(false);
    m_horizontal->addWidget(m_browser);
    m_horizontal->addWidget(m_vertical);
    m_horizontal->addWidget(m_detail);
    m_horizontal->setStretchFactor(0, 0);
    m_horizontal->setStretchFactor(1, 1);
    m_horizontal->setStretchFactor(2, 0);

    auto* layout = new QVBoxLayout(m_workspace);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    layout->addWidget(m_transport);
    layout->addWidget(m_horizontal, 1);

    connect(m_transport, &TransportBar::message, this, &MainWindow::showStatus);
    connect(m_transport, &TransportBar::sessionRequested, this, &MainWindow::showSession);
    connect(m_transport, &TransportBar::arrangementRequested, this, &MainWindow::showArrangement);
    connect(m_session, &SessionView::trackSelected, this, &MainWindow::selectTrack);
    connect(m_mixer, &MixerSection::trackSelected, this, &MainWindow::selectTrack);
    connect(m_session, &SessionView::slotClicked, this, [this](int track, int scene) {
        m_detail->showClipPage();
        showStatus(tr("Slot %1 on %2 is empty.").arg(scene + 1).arg(trackName(track)));
    });
    connect(m_browser, &BrowserPanel::fileActivated, this, [this](const QString& path) {
        showStatus(tr("Loading %1 is not available until the core imports media.").arg(path));
    });
    connect(m_bridge, &ProjectBridge::changed, this, [this] {
        if (m_detail->selectedTrack() >= 0) {
            const int index = qMin<int>(m_detail->selectedTrack(), static_cast<int>(m_bridge->trackCount()) - 1);
            m_detail->setSelectedTrack(index, trackName(index));
            m_session->selectTrack(index);
            m_mixer->selectTrack(index);
        }
        updateEditActions();
    });
}

QString MainWindow::trackName(int index) const
{
    return index >= 0 ? m_bridge->trackName(static_cast<quint64>(index)) : QString();
}

void MainWindow::renameSelectedTrack()
{
    const int index = m_detail->selectedTrack();
    if (index < 0) {
        showStatus(tr("Select a track to rename."));
        return;
    }
    bool ok = false;
    const QString name = QInputDialog::getText(this, tr("Rename Track"), tr("Name"), QLineEdit::Normal,
        trackName(index), &ok);
    if (!ok) {
        return;
    }
    if (!m_bridge->setTrackName(static_cast<quint64>(index), name)) {
        showStatus(tr("The core rejected that name."));
    }
}

void MainWindow::deleteSelectedTrack()
{
    const int index = m_detail->selectedTrack();
    if (index < 0) {
        showStatus(tr("Select a track to delete."));
        return;
    }
    if (!m_bridge->deleteTrack(static_cast<quint64>(index))) {
        showStatus(tr("Could not delete the track."));
        return;
    }
    selectTrack(qMin(index, static_cast<int>(m_bridge->trackCount()) - 1));
}

void MainWindow::updateEditActions()
{
    if (QAction* a = action(QStringLiteral("actionUndo"))) {
        a->setEnabled(m_bridge->canUndo());
    }
    if (QAction* a = action(QStringLiteral("actionRedo"))) {
        a->setEnabled(m_bridge->canRedo());
    }
    const bool hasSelection = m_detail->selectedTrack() >= 0;
    for (const char* name : {"actionRename", "actionDelete"}) {
        if (QAction* a = action(QLatin1String(name))) {
            a->setEnabled(hasSelection);
        }
    }
}

bool MainWindow::isStartScreenVisible() const
{
    return m_root->currentWidget() == m_start;
}

bool MainWindow::isSessionVisible() const
{
    return m_views->currentIndex() == 0;
}

QAction* MainWindow::action(const QString& objectName) const
{
    return findChild<QAction*>(objectName);
}

void MainWindow::showStartScreen()
{
    if (!isMaximized() && !isFullScreen()) {
        if (m_root->currentWidget() != m_start) {
            m_workspaceSize = size();
        }
        resize(840, 480);
    }
    statusBar()->clearMessage();
    m_start->reloadRecent();
    rebuildRecentMenu();
    m_root->setCurrentWidget(m_start);
    setWindowTitle(QStringLiteral("Nylon"));
}

void MainWindow::updateWindowTitle()
{
    const QString path = m_bridge->bundlePath();
    const QString name = path.isEmpty() ? tr("Untitled") : QFileInfo(path).completeBaseName();
    setWindowTitle(tr("%1 - Nylon").arg(name));
}

void MainWindow::rebuildRecentMenu()
{
    if (!m_recentMenu) {
        return;
    }
    const QList<QAction*> old = m_recentMenu->actions();
    for (QAction* a : old) {
        if (a->objectName() != QLatin1String("actionClearRecent") && !a->isSeparator()) {
            m_recentMenu->removeAction(a);
            a->deleteLater();
        }
    }
    const QStringList recent = StartScreen::recentProjects();
    QAction* clear = findChild<QAction*>(QStringLiteral("actionClearRecent"));
    QAction* before = m_recentMenu->actions().isEmpty() ? nullptr : m_recentMenu->actions().first();
    for (const QString& path : recent) {
        auto* a = new QAction(QFileInfo(path).completeBaseName(), m_recentMenu);
        a->setToolTip(path);
        a->setStatusTip(path);
        connect(a, &QAction::triggered, this, [this, path] { openProjectAt(path); });
        m_recentMenu->insertAction(before, a);
    }
    if (!recent.isEmpty() && before) {
        m_recentMenu->insertSeparator(before);
    }
    if (clear) {
        clear->setEnabled(!recent.isEmpty());
    }
}

void MainWindow::openProject()
{
    const QString dir = QFileDialog::getExistingDirectory(this, tr("Open Project"),
        QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation));
    if (!dir.isEmpty()) {
        openProjectAt(dir);
    }
}

bool MainWindow::openProjectAt(const QString& bundleDirectory)
{
    if (!m_bridge->open(bundleDirectory)) {
        showStatus(tr("Could not open %1: not a readable project bundle.").arg(QFileInfo(bundleDirectory).fileName()));
        return false;
    }
    StartScreen::addRecentProject(bundleDirectory);
    rebuildRecentMenu();
    selectTrack(-1);
    if (!isMaximized() && !isFullScreen() && m_root->currentWidget() == m_start) {
        resize(m_workspaceSize);
    }
    m_root->setCurrentWidget(m_workspace);
    updateWindowTitle();
    showSession();
    showStatus(tr("Opened %1.").arg(QFileInfo(bundleDirectory).completeBaseName()));
    return true;
}

bool MainWindow::saveProject()
{
    if (m_bridge->bundlePath().isEmpty()) {
        saveProjectAs();
        return !m_bridge->bundlePath().isEmpty();
    }
    return saveProjectTo(m_bridge->bundlePath());
}

void MainWindow::saveProjectAs()
{
    QString path = QFileDialog::getSaveFileName(this, tr("Save Project As"),
        QStandardPaths::writableLocation(QStandardPaths::DocumentsLocation) + QStringLiteral("/Untitled.nylon"),
        tr("Nylon project bundle (*.nylon)"));
    if (path.isEmpty()) {
        return;
    }
    if (!path.endsWith(QLatin1String(".nylon"), Qt::CaseInsensitive)) {
        path += QStringLiteral(".nylon");
    }
    saveProjectTo(path);
}

bool MainWindow::saveProjectTo(const QString& bundleDirectory)
{
    if (!m_bridge->save(bundleDirectory)) {
        showStatus(tr("Could not save to %1.").arg(bundleDirectory));
        return false;
    }
    StartScreen::addRecentProject(bundleDirectory);
    rebuildRecentMenu();
    updateWindowTitle();
    showStatus(tr("Saved %1.").arg(QFileInfo(bundleDirectory).completeBaseName()));
    return true;
}

void MainWindow::newProject()
{
    if (!m_bridge->reset()) {
        showStatus(tr("The core could not create a project."));
        return;
    }
    if (!isMaximized() && !isFullScreen() && m_root->currentWidget() == m_start) {
        resize(m_workspaceSize);
    }
    selectTrack(-1);
    m_root->setCurrentWidget(m_workspace);
    updateWindowTitle();
    showSession();
}

void MainWindow::showSession()
{
    m_root->setCurrentWidget(m_workspace);
    m_views->setCurrentIndex(0);
    m_transport->showSessionActive(true);
}

void MainWindow::showArrangement()
{
    m_root->setCurrentWidget(m_workspace);
    m_views->setCurrentIndex(1);
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

void MainWindow::selectTrack(int index)
{
    if (index >= static_cast<int>(m_bridge->trackCount())) {
        index = static_cast<int>(m_bridge->trackCount()) - 1;
    }
    m_session->selectTrack(index);
    m_mixer->selectTrack(index);
    m_detail->setSelectedTrack(index, index >= 0 ? trackName(index) : QString());
    updateEditActions();
}

QList<QAction*> MainWindow::namedActions() const
{
    QList<QAction*> out;
    for (QAction* a : findChildren<QAction*>()) {
        if (!a->objectName().isEmpty() && !a->isSeparator() && !a->text().isEmpty()) {
            out.append(a);
        }
    }
    return out;
}

void MainWindow::showPreferences()
{
    PreferencesDialog dialog(m_themes, namedActions(), this);
    connect(&dialog, &PreferencesDialog::libraryRootChanged, m_browser, &BrowserPanel::reload);
    dialog.exec();
}

void MainWindow::showCommandPalette()
{
    auto* palette = new CommandPalette(namedActions(), &m_themes->theme(), this);
    palette->setAttribute(Qt::WA_DeleteOnClose);
    const QPoint anchor = mapToGlobal(QPoint((width() - palette->width()) / 2, m_titleBar->height() + 8));
    palette->move(anchor);
    palette->show();
}

void MainWindow::closeEvent(QCloseEvent* event)
{
    saveLayout();
    QMainWindow::closeEvent(event);
}

void MainWindow::buildMenus()
{
    auto add = [this](QMenu* menu, const QString& name, const QString& text, const QKeySequence& shortcut,
                   const std::function<void()>& slot, bool enabled = true, const QString& why = QString()) {
        QAction* a = menu->addAction(text);
        a->setObjectName(name);
        a->setMenuRole(QAction::NoRole);
        if (!shortcut.isEmpty()) {
            a->setShortcut(shortcut);
        }
        a->setEnabled(enabled);
        if (!enabled && !why.isEmpty()) {
            a->setStatusTip(why);
            a->setToolTip(why);
        }
        if (slot) {
            connect(a, &QAction::triggered, this, slot);
        }
        return a;
    };
    const QString noPersist = tr("Not available until the core can read and write project bundles.");
    const QString noEdit = tr("Not available until the core exposes clip editing.");
    const QString noTransport = tr("Not available until an audio backend drives the transport.");

    auto roundMenu = [](QMenu* m) {
        m->setWindowFlag(Qt::FramelessWindowHint);
        m->setWindowFlag(Qt::NoDropShadowWindowHint);
        m->setAttribute(Qt::WA_TranslucentBackground);
        return m;
    };
    QMenuBar* bar = m_titleBar->menuBar();
    QMenu* file = roundMenu(bar->addMenu(tr("&File")));
    add(file, QStringLiteral("actionNew"), tr("&New Project"), QKeySequence::New, [this] { newProject(); });
    add(file, QStringLiteral("actionOpen"), tr("&Open..."), QKeySequence::Open, [this] { openProject(); },
        ProjectBridge::isPersistenceAvailable(), noPersist);
    m_recentMenu = roundMenu(file->addMenu(tr("Open &Recent")));
    m_recentMenu->setEnabled(ProjectBridge::isPersistenceAvailable());
    QAction* clearRecent = m_recentMenu->addAction(tr("Clear List"));
    clearRecent->setObjectName(QStringLiteral("actionClearRecent"));
    connect(clearRecent, &QAction::triggered, this, [this] {
        StartScreen::clearRecentProjects();
        m_start->reloadRecent();
        rebuildRecentMenu();
    });
    rebuildRecentMenu();
    add(file, QStringLiteral("actionClose"), tr("&Close Project"), QKeySequence(Qt::CTRL | Qt::Key_W), [this] { showStartScreen(); });
    file->addSeparator();
    add(file, QStringLiteral("actionSave"), tr("&Save"), QKeySequence::Save, [this] { saveProject(); },
        ProjectBridge::isPersistenceAvailable(), noPersist);
    add(file, QStringLiteral("actionSaveAs"), tr("Save &As..."), QKeySequence::SaveAs, [this] { saveProjectAs(); },
        ProjectBridge::isPersistenceAvailable(), noPersist);
    file->addSeparator();
    add(file, QStringLiteral("actionPreferences"), tr("&Preferences..."), QKeySequence(Qt::CTRL | Qt::Key_Comma),
        [this] { showPreferences(); });
    file->addSeparator();
    add(file, QStringLiteral("actionQuit"), tr("&Quit"), QKeySequence(Qt::CTRL | Qt::Key_Q), [this] { close(); });

    QMenu* edit = roundMenu(bar->addMenu(tr("&Edit")));
    add(edit, QStringLiteral("actionUndo"), tr("&Undo"), QKeySequence::Undo, [this] {
        if (!m_bridge->undo()) {
            showStatus(tr("Nothing to undo."));
        }
    });
    add(edit, QStringLiteral("actionRedo"), tr("&Redo"), QKeySequence::Redo, [this] {
        if (!m_bridge->redo()) {
            showStatus(tr("Nothing to redo."));
        }
    });
    edit->addSeparator();
    add(edit, QStringLiteral("actionCut"), tr("Cu&t"), QKeySequence::Cut, nullptr, false, noEdit);
    add(edit, QStringLiteral("actionCopy"), tr("&Copy"), QKeySequence::Copy, nullptr, false, noEdit);
    add(edit, QStringLiteral("actionPaste"), tr("&Paste"), QKeySequence::Paste, nullptr, false, noEdit);
    add(edit, QStringLiteral("actionDuplicate"), tr("&Duplicate"), QKeySequence(Qt::CTRL | Qt::Key_D), nullptr, false, noEdit);
    add(edit, QStringLiteral("actionDelete"), tr("De&lete Track"), QKeySequence::Delete, [this] { deleteSelectedTrack(); });
    edit->addSeparator();
    add(edit, QStringLiteral("actionSelectAll"), tr("Select &All"), QKeySequence::SelectAll, nullptr, false, noEdit);
    add(edit, QStringLiteral("actionRename"), tr("Re&name Track"), QKeySequence(Qt::CTRL | Qt::Key_R),
        [this] { renameSelectedTrack(); });

    QMenu* create = roundMenu(bar->addMenu(tr("&Create")));
    auto insert = [this](ProjectBridge::TrackKind kind) {
        if (!m_bridge->addTrack(kind)) {
            showStatus(tr("Could not add a track."));
            return;
        }
        m_root->setCurrentWidget(m_workspace);
        selectTrack(static_cast<int>(m_bridge->trackCount()) - 1);
    };
    add(create, QStringLiteral("actionAddTrack"), tr("Insert &Audio Track"), QKeySequence(Qt::CTRL | Qt::Key_T),
        [insert] { insert(ProjectBridge::TrackKind::Audio); });
    add(create, QStringLiteral("actionAddMidiTrack"), tr("Insert &MIDI Track"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_T),
        [insert] { insert(ProjectBridge::TrackKind::Midi); });
    add(create, QStringLiteral("actionAddReturnTrack"), tr("Insert &Return Track"), QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_T),
        [insert] { insert(ProjectBridge::TrackKind::Return); });
    create->addSeparator();
    add(create, QStringLiteral("actionInsertScene"), tr("Insert &Scene"), QKeySequence(Qt::CTRL | Qt::Key_I), nullptr, false,
        tr("Not available until the core exposes scenes."));
    add(create, QStringLiteral("actionInsertClip"), tr("Insert Empty MIDI &Clip"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_M),
        nullptr, false, noEdit);

    QMenu* view = roundMenu(bar->addMenu(tr("&View")));
    QAction* browserAction = add(view, QStringLiteral("actionToggleBrowser"), tr("&Browser"),
        QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_B), nullptr);
    browserAction->setCheckable(true);
    browserAction->setChecked(true);
    connect(browserAction, &QAction::toggled, m_browser, &QWidget::setVisible);
    QAction* detailAction = add(view, QStringLiteral("actionToggleDetail"), tr("&Detail View"),
        QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_L), nullptr);
    detailAction->setCheckable(true);
    detailAction->setChecked(true);
    connect(detailAction, &QAction::toggled, m_detail, &QWidget::setVisible);
    QAction* mixerAction = add(view, QStringLiteral("actionToggleMixer"), tr("&Mixer"),
        QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_M), nullptr);
    mixerAction->setCheckable(true);
    mixerAction->setChecked(true);
    connect(mixerAction, &QAction::toggled, m_mixer, &QWidget::setVisible);
    view->addSeparator();
    add(view, QStringLiteral("actionSession"), tr("&Session View"), QKeySequence(Qt::Key_F1), [this] { showSession(); });
    add(view, QStringLiteral("actionArrangement"), tr("&Arrangement View"), QKeySequence(Qt::Key_F2), [this] { showArrangement(); });
    add(view, QStringLiteral("actionToggleView"), tr("Toggle Session/Arrangement"), QKeySequence(Qt::Key_Tab), [this] { toggleView(); });
    view->addSeparator();
    QAction* fullScreen = add(view, QStringLiteral("actionFullScreen"), tr("&Full Screen"), QKeySequence::FullScreen, nullptr);
    fullScreen->setCheckable(true);
    connect(fullScreen, &QAction::toggled, this, [this](bool on) {
        if (on) {
            showFullScreen();
        } else {
            showNormal();
        }
    });
    view->addSeparator();
    QMenu* themeMenu = roundMenu(view->addMenu(tr("&Theme")));
    m_themeActions = new QActionGroup(this);
    m_themeActions->setExclusive(true);
    for (const QString& name : ThemeManager::builtinNames()) {
        QString label = name;
        label[0] = label[0].toUpper();
        QAction* a = themeMenu->addAction(label);
        a->setCheckable(true);
        a->setData(name);
        a->setChecked(name == m_themes->currentName());
        m_themeActions->addAction(a);
        connect(a, &QAction::triggered, this, [this, name] {
            if (m_themes->load(name)) {
                QSettings().setValue(QStringLiteral("look/theme"), name);
            }
        });
    }
    themeMenu->addSeparator();
    themeMenu->addAction(tr("&Reload Theme"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_R), this, [this] {
        if (m_themes->reload()) {
            showStatus(tr("Theme reloaded."));
        }
    });

    QMenu* transport = roundMenu(bar->addMenu(tr("&Transport")));
    add(transport, QStringLiteral("actionPlay"), tr("&Play"), QKeySequence(Qt::Key_Space), nullptr,
        ProjectBridge::isTransportAvailable(), noTransport);
    add(transport, QStringLiteral("actionStop"), tr("&Stop"), QKeySequence(Qt::SHIFT | Qt::Key_Space), nullptr,
        ProjectBridge::isTransportAvailable(), noTransport);
    add(transport, QStringLiteral("actionRecord"), tr("&Record"), QKeySequence(Qt::Key_F9), nullptr,
        ProjectBridge::isTransportAvailable(), noTransport);
    transport->addSeparator();
    add(transport, QStringLiteral("actionLoop"), tr("&Loop"), QKeySequence(Qt::CTRL | Qt::Key_L), nullptr,
        ProjectBridge::isTransportAvailable(), noTransport);
    add(transport, QStringLiteral("actionMetronome"), tr("&Metronome"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_K), nullptr,
        ProjectBridge::isTransportAvailable(), noTransport);

    view->addSeparator();
    QAction* paletteAction = add(view, QStringLiteral("actionCommandPalette"), tr("&Command Palette..."),
        QKeySequence(Qt::CTRL | Qt::Key_K), [this] { showCommandPalette(); });
    paletteAction->setShortcuts({QKeySequence(Qt::CTRL | Qt::Key_K), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_P)});
    paletteAction->setStatusTip(tr("Search every command by name."));

    QMenu* help = roundMenu(bar->addMenu(tr("&Help")));
    add(help, QStringLiteral("actionLibraryFolder"), tr("Show &Library Folder"), QKeySequence(), [this] {
        showStatus(BrowserPanel::libraryRoot());
    });
    add(help, QStringLiteral("actionAbout"), tr("&About Nylon"), QKeySequence(), [this] {
        QMessageBox::about(this, tr("About Nylon"),
            tr("Nylon %1\nDigital audio workstation.\nCopyright (c) Nylon Contributors")
                .arg(QCoreApplication::applicationVersion()));
    });
}

void MainWindow::applyTheme(const Theme& theme)
{
    qApp->setStyleSheet(theme.styleSheet());
    m_titleBar->setTheme(&theme);
    const int gap = qBound(0, theme.metricInt(QStringLiteral("panel.gap"), 6), 32);
    if (auto* layout = m_workspace->layout()) {
        layout->setContentsMargins(gap, gap, gap, gap);
        layout->setSpacing(gap);
    }
    m_start->setTheme(&theme);
    m_transport->setTheme(&theme);
    m_browser->setTheme(&theme);
    m_session->setTheme(&theme);
    m_mixer->setTheme(&theme);
    m_arrangement->setTheme(&theme);
    m_detail->setTheme(&theme);
    m_horizontal->setHandleWidth(gap);
    m_vertical->setHandleWidth(gap);
    if (m_themeActions) {
        for (QAction* a : m_themeActions->actions()) {
            a->setChecked(a->data().toString() == m_themes->currentName());
        }
    }
}

void MainWindow::showStatus(const QString& text)
{
    statusBar()->showMessage(text, 5000);
}

void MainWindow::restoreLayout()
{
    QSettings settings;
    settings.beginGroup(QStringLiteral("layout"));
    const QByteArray geometry = settings.value(QStringLiteral("geometry")).toByteArray();
    if (!geometry.isEmpty()) {
        restoreGeometry(geometry);
    }
    const QByteArray horizontal = settings.value(QStringLiteral("horizontalV2")).toByteArray();
    if (horizontal.isEmpty() || !m_horizontal->restoreState(horizontal)) {
        const int w = m_themes->theme().metricInt(QStringLiteral("browser.width"), 230);
        const int detail = m_themes->theme().metricInt(QStringLiteral("detail.width"), 260);
        m_horizontal->setSizes({w, qMax(400, width() - w - detail), detail});
    }
    const QByteArray vertical = settings.value(QStringLiteral("verticalV2")).toByteArray();
    if (vertical.isEmpty() || !m_vertical->restoreState(vertical)) {
        const int h = m_themes->theme().metricInt(QStringLiteral("mixer.height"), 170);
        m_vertical->setSizes({qMax(300, height() - h), h});
    }
    for (const char* name : {"actionToggleBrowser", "actionToggleDetail", "actionToggleMixer"}) {
        if (QAction* a = action(QLatin1String(name))) {
            a->setChecked(settings.value(QLatin1String(name), true).toBool());
        }
    }
    settings.endGroup();
}

void MainWindow::saveLayout()
{
    QSettings settings;
    settings.beginGroup(QStringLiteral("layout"));
    settings.setValue(QStringLiteral("geometry"), saveGeometry());
    settings.setValue(QStringLiteral("horizontalV2"), m_horizontal->saveState());
    settings.setValue(QStringLiteral("verticalV2"), m_vertical->saveState());
    for (const char* name : {"actionToggleBrowser", "actionToggleDetail", "actionToggleMixer"}) {
        if (QAction* a = action(QLatin1String(name))) {
            settings.setValue(QLatin1String(name), a->isChecked());
        }
    }
    settings.endGroup();
}

} // namespace nylon
