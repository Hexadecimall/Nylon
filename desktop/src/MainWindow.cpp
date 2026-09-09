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
#include "widgets/FlatButton.h"

#include <QAction>
#include <QActionGroup>
#include <QApplication>
#include <QCloseEvent>
#include <QFileDialog>
#include <QFileInfo>
#include <QInputDialog>
#include <QHBoxLayout>
#include <QStandardPaths>
#include <QKeySequence>
#include <QLabel>
#include <QLineEdit>
#include <QMenu>
#include <QMenuBar>
#include <QMessageBox>
#include <QScrollBar>
#include <QSettings>

#include <cmath>
#include <QSignalBlocker>
#include <QSplitter>
#include <QStackedWidget>
#include <QStatusBar>
#include <QVBoxLayout>
#include <QWindow>
#include <QPainter>
#include <QPainterPath>
#include <QMouseEvent>

namespace nylon {

namespace {
// Smallest window each screen is laid out for. The workspace floor is
// whatever its own layout needs, never less than this.
const QSize kWorkspaceFloor(960, 640);
const QSize kStartMinimum(720, 420);
} // namespace

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
    connect(m_start, &StartScreen::templateRequested, this, &MainWindow::newProjectFromTemplate);
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

    m_lowerViews = new QStackedWidget(m_workspace);
    m_lowerViews->addWidget(m_mixer);
    m_lowerViews->addWidget(m_detail);
    m_lowerViews->setCurrentWidget(m_mixer);

    m_lowerDock = new QWidget(m_workspace);
    m_lowerDock->setObjectName(QStringLiteral("lowerDock"));
    auto* lowerHeader = new QWidget(m_lowerDock);
    lowerHeader->setObjectName(QStringLiteral("lowerDockHeader"));
    m_mixerTab = new FlatButton(theme, lowerHeader);
    m_mixerTab->setText(tr("Mixer"));
    m_mixerTab->setCheckable(true);
    m_mixerTab->setChecked(true);
    m_detailTab = new FlatButton(theme, lowerHeader);
    m_detailTab->setText(tr("Editor"));
    m_detailTab->setCheckable(true);
    m_lowerClose = new FlatButton(theme, lowerHeader);
    m_lowerClose->setText(tr("Hide"));
    auto* lowerHeaderLayout = new QHBoxLayout(lowerHeader);
    lowerHeaderLayout->setContentsMargins(4, 3, 4, 3);
    lowerHeaderLayout->setSpacing(3);
    lowerHeaderLayout->addWidget(m_mixerTab);
    lowerHeaderLayout->addWidget(m_detailTab);
    lowerHeaderLayout->addStretch(1);
    lowerHeaderLayout->addWidget(m_lowerClose);
    auto* lowerLayout = new QVBoxLayout(m_lowerDock);
    lowerLayout->setContentsMargins(0, 0, 0, 0);
    lowerLayout->setSpacing(0);
    lowerLayout->addWidget(lowerHeader);
    lowerLayout->addWidget(m_lowerViews, 1);

    m_vertical = new QSplitter(Qt::Vertical, m_workspace);
    m_vertical->setObjectName(QStringLiteral("verticalSplit"));
    m_vertical->setChildrenCollapsible(true);
    m_vertical->addWidget(m_views);
    m_vertical->addWidget(m_lowerDock);
    m_vertical->setStretchFactor(0, 1);
    m_vertical->setStretchFactor(1, 0);

    m_horizontal = new QSplitter(Qt::Horizontal, m_workspace);
    m_horizontal->setObjectName(QStringLiteral("horizontalSplit"));
    m_horizontal->setChildrenCollapsible(false);
    m_horizontal->addWidget(m_browser);
    m_horizontal->addWidget(m_vertical);
    m_horizontal->setStretchFactor(0, 0);
    m_horizontal->setStretchFactor(1, 1);

    auto* layout = new QVBoxLayout(m_workspace);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    layout->addWidget(m_transport);
    layout->addWidget(m_horizontal, 1);
    auto* viewBar = new QWidget(m_workspace);
    viewBar->setObjectName(QStringLiteral("viewBar"));
    m_browserToggle = new FlatButton(theme, viewBar);
    m_browserToggle->setText(tr("Browser"));
    m_browserToggle->setCheckable(true);
    m_browserToggle->setChecked(true);
    m_mixerToggle = new FlatButton(theme, viewBar);
    m_mixerToggle->setText(tr("Mixer"));
    m_mixerToggle->setCheckable(true);
    m_editorToggle = new FlatButton(theme, viewBar);
    m_editorToggle->setText(tr("Editor"));
    m_editorToggle->setCheckable(true);
    auto* viewBarLayout = new QHBoxLayout(viewBar);
    viewBarLayout->setContentsMargins(4, 2, 4, 2);
    viewBarLayout->setSpacing(3);
    m_workspaceContext = new QLabel(viewBar);
    m_workspaceContext->setObjectName(QStringLiteral("workspaceContext"));
    viewBarLayout->addWidget(m_workspaceContext);
    viewBarLayout->addStretch(1);
    m_engineStatus = new QLabel(viewBar);
    m_engineStatus->setObjectName(QStringLiteral("workspaceContext"));
    m_engineStatus->setStatusTip(tr("Output device, sample rate, block size and dropouts."));
    viewBarLayout->addWidget(m_engineStatus);
    viewBarLayout->addSpacing(8);
    viewBarLayout->addWidget(m_browserToggle);
    viewBarLayout->addWidget(m_editorToggle);
    viewBarLayout->addWidget(m_mixerToggle);
    layout->addWidget(viewBar);
    m_lowerDock->hide();

    connect(m_browserToggle, &FlatButton::clicked, this, [this] {
        if (QAction* item = action(QStringLiteral("actionToggleBrowser"))) item->toggle();
    });
    connect(m_mixerToggle, &FlatButton::clicked, this, [this] {
        if (QAction* item = action(QStringLiteral("actionToggleMixer"))) item->toggle();
    });
    connect(m_editorToggle, &FlatButton::clicked, this, [this] {
        if (QAction* item = action(QStringLiteral("actionToggleDetail"))) item->toggle();
    });

    connect(m_transport, &TransportBar::message, this, &MainWindow::showStatus);
    connect(m_transport, &TransportBar::sessionRequested, this, &MainWindow::showSession);
    connect(m_transport, &TransportBar::arrangementRequested, this, &MainWindow::showArrangement);
    connect(m_transport, &TransportBar::playRequested, this, &MainWindow::startPlayback);
    connect(m_transport, &TransportBar::stopRequested, this, &MainWindow::stopPlayback);
    connect(m_arrangement, &ArrangementView::clipRequested, this,
        [this](int track, double start, double length) {
            createArrangementClip(track, start, length);
        });
    connect(m_arrangement, &ArrangementView::addTrackRequested, this, [this] {
        if (QAction* add = action(QStringLiteral("actionAddTrack"))) {
            add->trigger();
        }
    });
    connect(m_arrangement, &ArrangementView::locateRequested, this, [this](double beats) {
        m_bridge->locate(beats);
        m_transport->showPosition(beats);
    });
    connect(m_mixerTab, &FlatButton::clicked, this, [this] { showLowerWidget(m_mixer); });
    connect(m_detailTab, &FlatButton::clicked, this, [this] { showLowerWidget(m_detail); });
    connect(m_lowerClose, &FlatButton::clicked, this, [this] {
        m_lowerDock->hide();
        if (QAction* action = this->action(QStringLiteral("actionToggleMixer"))) action->setChecked(false);
        if (QAction* action = this->action(QStringLiteral("actionToggleDetail"))) action->setChecked(false);
    });
    connect(m_session, &SessionView::trackSelected, this, &MainWindow::selectTrack);
    connect(m_arrangement, &ArrangementView::trackSelected, this, &MainWindow::selectTrack);
    connect(m_mixer, &MixerSection::trackSelected, this, &MainWindow::selectTrack);
    connect(m_session, &SessionView::slotClicked, this, [this](int track, int scene) {
        m_detail->setSelectedClip(track, scene);
        m_detail->showClipPage();
        showLowerWidget(m_detail);
        if (m_bridge->clipSlotOccupied(static_cast<quint64>(track), static_cast<quint64>(scene))) {
            showStatus(tr("Selected %1.").arg(m_bridge->clipName(static_cast<quint64>(track), static_cast<quint64>(scene))));
        } else {
            showStatus(tr("Empty slot. Double-click to create a MIDI clip."));
        }
    });
    connect(m_session, &SessionView::slotCreateRequested, this, [this](int track, int scene) {
        if (m_bridge->trackKind(static_cast<quint64>(track)) != ProjectBridge::TrackKind::Midi) {
            showStatus(tr("MIDI clips require a MIDI track."));
            return;
        }
        if (m_bridge->clipSlotOccupied(static_cast<quint64>(track), static_cast<quint64>(scene))) {
            showStatus(tr("The slot already contains a clip."));
            return;
        }
        if (m_bridge->createMidiClip(static_cast<quint64>(track), static_cast<quint64>(scene), 4.0)) {
            m_detail->setSelectedClip(track, scene);
            m_detail->showClipPage();
            showLowerWidget(m_detail);
            showStatus(tr("Created MIDI clip."));
        }
    });
    connect(m_browser, &BrowserPanel::categoryDetached, this, &MainWindow::openCategoryWindow);
    connect(m_browser, &BrowserPanel::fileActivated, this, [this](const QString& path) {
        showStatus(tr("Loading %1 is not available until the core imports media.").arg(path));
    });
    connect(m_bridge, &ProjectBridge::changed, this, [this] {
        if (m_detail->selectedTrack() >= 0) {
            const int index = qMin<int>(m_detail->selectedTrack(), static_cast<int>(m_bridge->trackCount()) - 1);
            const int clipTrack = m_detail->selectedClipTrack();
            const int clipScene = m_detail->selectedClipScene();
            m_detail->setSelectedTrack(index, trackName(index));
            {
                const QSignalBlocker sessionBlock(m_session);
                const QSignalBlocker arrangementBlock(m_arrangement);
                const QSignalBlocker mixerBlock(m_mixer);
                m_session->selectTrack(index);
                m_arrangement->selectTrack(index);
                m_mixer->selectTrack(index);
            }
            if (clipTrack >= 0 && clipScene >= 0
                && m_bridge->clipSlotOccupied(static_cast<quint64>(clipTrack), static_cast<quint64>(clipScene))) {
                m_detail->setSelectedClip(clipTrack, clipScene);
            } else {
                m_detail->setSelectedClip(index, -1);
            }
        }
        updateEditActions();
        updateWorkspaceContext();
    });
}

void MainWindow::showLowerWidget(QWidget* widget)
{
    m_lowerViews->setCurrentWidget(widget);
    m_lowerDock->show();
    m_mixerTab->setChecked(widget == m_mixer);
    m_detailTab->setChecked(widget == m_detail);
    const int height = m_themes->theme().metricInt(
        widget == m_mixer ? QStringLiteral("mixer.height") : QStringLiteral("detail.height"), 210);
    m_vertical->setSizes({qMax(300, m_vertical->height() - height), height});
    if (QAction* action = this->action(QStringLiteral("actionToggleMixer"))) {
        const QSignalBlocker block(action);
        action->setChecked(widget == m_mixer);
    }
    if (QAction* action = this->action(QStringLiteral("actionToggleDetail"))) {
        const QSignalBlocker block(action);
        action->setChecked(widget == m_detail);
    }
}

void MainWindow::hideLowerWidget(QWidget* widget)
{
    if (m_lowerViews->currentWidget() == widget) m_lowerDock->hide();
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

bool MainWindow::isLowerDockVisible() const
{
    return m_lowerDock->isVisibleTo(m_workspace);
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
        // The start screen is a small window, so the workspace minimum has
        // to come off before the resize can take effect.
        setMinimumSize(kStartMinimum);
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
    enterWorkspace();
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
    selectTrack(-1);
    enterWorkspace();
    updateWindowTitle();
    showSession();
}

void MainWindow::newProjectFromTemplate(int audioTracks, int midiTracks)
{
    newProject();
    for (int i = 0; i < audioTracks; ++i) {
        if (!m_bridge->addTrack(ProjectBridge::TrackKind::Audio)) {
            showStatus(tr("The core could not add an audio track."));
            return;
        }
    }
    for (int i = 0; i < midiTracks; ++i) {
        if (!m_bridge->addTrack(ProjectBridge::TrackKind::Midi)) {
            showStatus(tr("The core could not add a MIDI track."));
            return;
        }
    }
    if (m_bridge->trackCount() > 0) {
        selectTrack(0);
    }
    showArrangement();
}

void MainWindow::openAudio()
{
    if (m_bridge->isAudioOpen()) {
        return;
    }
    if (!m_bridge->openAudio()) {
        showStatus(tr("No audio output was available, so playback is off."));
        return;
    }
    showStatus(tr("Audio output: %1.").arg(m_bridge->audioDeviceName()));
    if (!m_audioPoll) {
        m_audioPoll = new QTimer(this);
        // Fast enough that the playhead reads as motion rather than as a
        // series of jumps, slow enough to leave the audio thread alone.
        m_audioPoll->setInterval(33);
        connect(m_audioPoll, &QTimer::timeout, this, &MainWindow::pollAudio);
    }
    m_audioPoll->start();
}

int MainWindow::createArrangementClip(int track, double startBeats, double lengthBeats)
{
    if (track < 0 || static_cast<quint64>(track) >= m_bridge->trackCount()) {
        return -1;
    }
    const quint64 index = static_cast<quint64>(track);
    if (m_bridge->trackKind(index) != ProjectBridge::TrackKind::Midi) {
        showStatus(tr("Only a MIDI track can hold a clip until audio recording exists."));
        return -1;
    }
    // A clip lives in a slot and is placed on the timeline from there, so
    // the first free slot on this track carries the new one.
    int scene = -1;
    const quint64 scenes = m_bridge->sceneCount();
    for (quint64 candidate = 0; candidate < scenes; ++candidate) {
        if (!m_bridge->clipSlotOccupied(index, candidate)) {
            scene = static_cast<int>(candidate);
            break;
        }
    }
    if (scene < 0) {
        if (!m_bridge->createScene(tr("Scene %1").arg(scenes + 1))) {
            showStatus(tr("The core would not add a scene for the clip."));
            return -1;
        }
        scene = static_cast<int>(scenes);
    }
    if (!m_bridge->createMidiClip(index, static_cast<quint64>(scene), lengthBeats)) {
        showStatus(tr("The core rejected the clip."));
        return -1;
    }
    BeatRange range {};
    range.startBeats = startBeats;
    range.lengthBeats = lengthBeats;
    if (!m_bridge->addArrangementClipFromSlot(index, static_cast<quint64>(scene), range)) {
        showStatus(tr("The clip was made but would not go on the timeline."));
        return -1;
    }
    selectTrack(track);
    m_detail->setSelectedClip(track, scene);
    m_detail->showClipPage();
    showLowerWidget(m_detail);
    showStatus(tr("Clip added. Draw notes in the editor, then press play."));
    return scene;
}

void MainWindow::openCategoryWindow(const QString& category)
{
    if (BrowserPanel::folderForCategory(category).isEmpty()) {
        return;
    }
    if (QWidget* open = m_categoryWindows.value(category)) {
        open->raise();
        open->activateWindow();
        return;
    }
    // A category window is a browser of its own, so it searches and loads
    // exactly like the panel in the workspace.
    auto* window = new QWidget(this, Qt::Window);
    window->setAttribute(Qt::WA_DeleteOnClose);
    window->setWindowTitle(tr("%1 - Nylon").arg(category));
    auto* panel = new BrowserPanel(&m_themes->theme(), window);
    panel->selectCategory(category);
    connect(panel, &BrowserPanel::fileActivated, this, [this](const QString& path) {
        showStatus(tr("Loading %1 is not available until the core imports media.").arg(path));
    });
    connect(m_themes, &ThemeManager::themeChanged, panel, [panel](const Theme& theme) {
        panel->setTheme(&theme);
    });
    auto* layout = new QVBoxLayout(window);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->addWidget(panel);
    window->resize(420, 560);
    connect(window, &QObject::destroyed, this, [this, category] { m_categoryWindows.remove(category); });
    m_categoryWindows.insert(category, window);
    window->show();
}

void MainWindow::updateEngineStatus()
{
    if (!m_engineStatus) {
        return;
    }
    if (!m_bridge->isAudioOpen()) {
        m_engineStatus->setText(tr("AUDIO OFF"));
        return;
    }
    AudioConfig config {};
    if (!m_bridge->audioConfig(config)) {
        m_engineStatus->setText(m_bridge->audioDeviceName().toUpper());
        return;
    }
    const quint64 dropped = m_bridge->audioDropouts();
    const QString rate = tr("%1 kHz").arg(config.sampleRate / 1000.0, 0, 'f', 1);
    const QString base = QStringLiteral("%1  %2  %3")
                             .arg(m_bridge->audioDeviceName().toUpper(), rate,
                                 tr("%1 frames").arg(config.blockFrames));
    m_engineStatus->setText(dropped == 0 ? base : base + tr("  %1 dropped").arg(dropped));
}

void MainWindow::startPlayback()
{
    // The bridge opens the device on the first play; the window only has
    // to start following it once that has happened.
    if (!m_bridge->play()) {
        showStatus(tr("The engine would not start."));
        return;
    }
    openAudio();
}

void MainWindow::stopPlayback()
{
    m_bridge->stop();
}

void MainWindow::pollAudio()
{
    if (!m_bridge->isAudioOpen()) {
        return;
    }
    m_bridge->reconcileTransport();
    updateEngineStatus();
    const double beats = m_bridge->positionBeats();
    m_arrangement->setPlayheadBeats(beats);
    m_transport->showPosition(beats);
    m_transport->playButton()->setChecked(m_bridge->isPlaying());

    // Levels arrive as amplitudes; the meters read decibels.
    const auto decibels = [](float amplitude) {
        return amplitude > 0.0f ? 20.0 * std::log10(static_cast<double>(amplitude)) : -120.0;
    };
    for (int index = 0; index < m_mixer->stripCount(); ++index) {
        Levels levels {};
        if (m_bridge->trackLevels(static_cast<quint64>(index), levels)) {
            m_mixer->setTrackLevels(index, decibels(levels.peakLeft), decibels(levels.peakRight),
                decibels(levels.rmsLeft), decibels(levels.rmsRight));
            m_arrangement->setTrackLevel(index,
                qMax(decibels(levels.peakLeft), decibels(levels.peakRight)));
        }
    }
    Levels master {};
    if (m_bridge->masterLevels(master)) {
        m_mixer->setMasterLevels(decibels(master.peakLeft), decibels(master.peakRight),
            decibels(master.rmsLeft), decibels(master.rmsRight));
    }
}

void MainWindow::enterWorkspace()
{
    if (m_root->currentWidget() == m_workspace) {
        return;
    }
    // The workspace carries the transport bar, the browser and the panels
    // side by side. Below this size they start to overlap each other, so
    // the window refuses to go smaller once a project is open.
    const QSize floor = m_workspace->minimumSizeHint().expandedTo(kWorkspaceFloor);
    setMinimumSize(floor);
    if (!isMaximized() && !isFullScreen()) {
        resize(m_workspaceSize.expandedTo(floor));
    }
    m_root->setCurrentWidget(m_workspace);
    // The controls come alive when the machine has an output to play to;
    // the stream itself waits for the first play.
    const bool available = ProjectBridge::hasAudioOutput();
    m_transport->setTransportAvailable(available);
    updateEngineStatus();
    for (const char* name : {"actionPlay", "actionStop"}) {
        if (QAction* item = action(QLatin1String(name))) {
            item->setEnabled(available);
            item->setToolTip(available ? QString() : item->toolTip());
        }
    }
}

void MainWindow::showSession()
{
    enterWorkspace();
    m_views->setCurrentIndex(0);
    m_transport->showSessionActive(true);
    updateWorkspaceContext();
}

void MainWindow::showArrangement()
{
    enterWorkspace();
    m_views->setCurrentIndex(1);
    m_transport->showSessionActive(false);
    updateWorkspaceContext();
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
    m_arrangement->selectTrack(index);
    m_mixer->selectTrack(index);
    m_detail->setSelectedTrack(index, index >= 0 ? trackName(index) : QString());
    m_detail->setSelectedClip(index, -1);
    updateEditActions();
    updateWorkspaceContext();
}

void MainWindow::updateWorkspaceContext()
{
    if (!m_workspaceContext) return;
    const QString view = isSessionVisible() ? tr("SESSION") : tr("ARRANGEMENT");
    const int selected = m_detail ? m_detail->selectedTrack() : -1;
    m_workspaceContext->setText(selected >= 0 ? tr("%1 / %2").arg(view, trackName(selected))
                                              : tr("%1 / NO TRACK SELECTED").arg(view));
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
            a->setShortcutVisibleInContextMenu(true);
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
        enterWorkspace();
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
    connect(browserAction, &QAction::toggled, m_browserToggle, &FlatButton::setChecked);
    QAction* detailAction = add(view, QStringLiteral("actionToggleDetail"), tr("&Detail View"),
        QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_L), nullptr);
    detailAction->setCheckable(true);
    detailAction->setChecked(false);
    connect(detailAction, &QAction::toggled, this, [this](bool on) {
        m_editorToggle->setChecked(on);
        if (on) showLowerWidget(m_detail);
        else hideLowerWidget(m_detail);
    });
    QAction* mixerAction = add(view, QStringLiteral("actionToggleMixer"), tr("&Mixer"),
        QKeySequence(Qt::CTRL | Qt::ALT | Qt::Key_M), nullptr);
    mixerAction->setCheckable(true);
    mixerAction->setChecked(false);
    connect(mixerAction, &QAction::toggled, this, [this](bool on) {
        m_mixerToggle->setChecked(on);
        if (on) showLowerWidget(m_mixer);
        else hideLowerWidget(m_mixer);
    });
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
    // Playback is offered when the machine has an output to play to; the
    // stream itself opens on the first play.
    const bool driven = ProjectBridge::hasAudioOutput();
    add(transport, QStringLiteral("actionPlay"), tr("&Play"), QKeySequence(Qt::Key_Space),
        [this] { startPlayback(); }, driven, noTransport);
    add(transport, QStringLiteral("actionStop"), tr("&Stop"), QKeySequence(Qt::SHIFT | Qt::Key_Space),
        [this] { stopPlayback(); }, driven, noTransport);
    add(transport, QStringLiteral("actionRecord"), tr("&Record"), QKeySequence(Qt::Key_F9), nullptr,
        false, noTransport);
    transport->addSeparator();
    add(transport, QStringLiteral("actionLoop"), tr("&Loop"), QKeySequence(Qt::CTRL | Qt::Key_L), nullptr,
        false, noTransport);
    add(transport, QStringLiteral("actionMetronome"), tr("&Metronome"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_K), nullptr,
        false, noTransport);

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
    const int border = qBound(0, theme.metricInt(QStringLiteral("window.border"), 1), 4);
    setContentsMargins(border, border, border, border);
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
    m_mixerTab->setTheme(&theme);
    m_detailTab->setTheme(&theme);
    m_lowerClose->setTheme(&theme);
    m_browserToggle->setTheme(&theme);
    m_mixerToggle->setTheme(&theme);
    m_editorToggle->setTheme(&theme);
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
    settings.beginGroup(QStringLiteral("layoutV3"));
    const QByteArray geometry = settings.value(QStringLiteral("geometry")).toByteArray();
    if (!geometry.isEmpty()) {
        restoreGeometry(geometry);
    }
    const QByteArray horizontal = settings.value(QStringLiteral("horizontalV3")).toByteArray();
    if (horizontal.isEmpty() || !m_horizontal->restoreState(horizontal)) {
        const int w = m_themes->theme().metricInt(QStringLiteral("browser.width"), 230);
        m_horizontal->setSizes({w, qMax(400, width() - w)});
    }
    const QByteArray vertical = settings.value(QStringLiteral("verticalV3")).toByteArray();
    if (vertical.isEmpty() || !m_vertical->restoreState(vertical)) {
        const int h = m_themes->theme().metricInt(QStringLiteral("mixer.height"), 170);
        m_vertical->setSizes({qMax(300, height() - h), h});
    }
    for (const char* name : {"actionToggleBrowser", "actionToggleDetail", "actionToggleMixer"}) {
        if (QAction* a = action(QLatin1String(name))) {
            const bool fallback = QLatin1String(name) == QLatin1String("actionToggleBrowser");
            a->setChecked(settings.value(QLatin1String(name), fallback).toBool());
        }
    }
    settings.endGroup();
}

void MainWindow::saveLayout()
{
    QSettings settings;
    settings.beginGroup(QStringLiteral("layoutV3"));
    settings.setValue(QStringLiteral("geometry"), saveGeometry());
    settings.setValue(QStringLiteral("horizontalV3"), m_horizontal->saveState());
    settings.setValue(QStringLiteral("verticalV3"), m_vertical->saveState());
    for (const char* name : {"actionToggleBrowser", "actionToggleDetail", "actionToggleMixer"}) {
        if (QAction* a = action(QLatin1String(name))) {
            settings.setValue(QLatin1String(name), a->isChecked());
        }
    }
    settings.endGroup();
}

} // namespace nylon
