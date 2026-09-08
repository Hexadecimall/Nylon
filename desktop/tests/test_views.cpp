#include "ArrangementView.h"
#include "BrowserPanel.h"
#include "DetailPanel.h"
#include "MainWindow.h"
#include "MixerSection.h"
#include "MixerStrip.h"
#include "StartScreen.h"
#include "TitleBar.h"
#include "widgets/Fader.h"
#include "widgets/FlatButton.h"
#include "widgets/Knob.h"
#include "widgets/ValueBox.h"
#include "ProjectBridge.h"
#include "SessionView.h"
#include "ThemeManager.h"
#include "TransportBar.h"

#include <QAction>
#include <QAction>
#include <QMenuBar>
#include <QStatusBar>
#include <QDir>
#include <QLineEdit>
#include <QMenu>
#include <QTemporaryDir>
#include <QListWidget>
#include <QFile>
#include <QScrollBar>
#include <QStandardPaths>
#include <QtTest>

#include <cmath>

#include "LayoutMath.h"

using namespace nylon;

class TestViews : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void emptyStateWhenNoTracks();
    void addTrackButtonGrowsBothViews();
    void undoRedoFromButtonsAndActions();
    void tempoBoxCommitsAndRevertsOnRejection();
    void viewSwitchButtonsAndToggle();
    void themeSwitchRepaintsWithNewTokens();
    void slotAndLaneGeometryFollowMetrics();
    void extremeMetricsStayWithinIntRange();
    void transportSpacingFollowsTokens();
    void startScreenThenWorkspace();
    void menusAreInWindowAndComplete();
    void selectionFlowsBetweenGridMixerAndDetail();
    void browserShowsLibraryCategories();
    void framelessWindowWithTitleBar();
    void saveOpenAndRecentThroughTheWindow();

private:
    ThemeManager m_themes;
};

void TestViews::initTestCase()
{
    QVERIFY(m_themes.load(QStringLiteral("nylon")));
}

void TestViews::emptyStateWhenNoTracks()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    QVERIFY(w.sessionView()->isShowingEmptyState());
    QVERIFY(w.arrangementView()->isShowingEmptyState());
    QCOMPARE(w.sessionView()->columnCount(), 0);
    QCOMPARE(w.arrangementView()->laneCount(), 0);
    QVERIFY(w.sessionView()->slotRect(0, 0).isEmpty());
    QVERIFY(w.arrangementView()->laneRect(0).isEmpty());
    // Painting the empty state must not fail.
    const QImage img = w.sessionView()->viewport()->grab().toImage();
    QVERIFY(!img.isNull());
    // Inside the rounded panel outline the grid shows the panel color.
    QCOMPARE(img.pixelColor(12, img.height() - 12), m_themes.theme().color(QStringLiteral("panel")));
}

void TestViews::addTrackButtonGrowsBothViews()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    QCOMPARE(bridge.trackCount(), 3ull);
    QCOMPARE(w.sessionView()->columnCount(), 3);
    QCOMPARE(w.arrangementView()->laneCount(), 3);
    QVERIFY(!w.sessionView()->isShowingEmptyState());
    QVERIFY(!w.arrangementView()->isShowingEmptyState());

    // The third column's header carries the third track color.
    const QRect slot = w.sessionView()->slotRect(2, 0);
    QVERIFY(!slot.isEmpty());
    const QImage img = w.sessionView()->viewport()->grab().toImage();
    // The color band sits just inside the panel outline.
    QCOMPARE(img.pixelColor(slot.center().x(), 1), m_themes.theme().trackColor(bridge.trackColorIndex(2)));
    QCOMPARE(img.pixelColor(slot.center()), m_themes.theme().color(QStringLiteral("session.slot")));
}

void TestViews::undoRedoFromButtonsAndActions()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    // With nothing to undo the action is disabled rather than reporting.
    QVERIFY(!w.action(QStringLiteral("actionUndo"))->isEnabled());
    w.action(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(bridge.trackCount(), 0ull);

    w.findChild<QAction*>(QStringLiteral("actionAddTrack"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
    w.findChild<QAction*>(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(bridge.trackCount(), 0ull);
    QCOMPARE(w.sessionView()->columnCount(), 0);
    w.findChild<QAction*>(QStringLiteral("actionRedo"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
    w.action(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(bridge.trackCount(), 0ull);
    w.action(QStringLiteral("actionRedo"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
    QVERIFY(!w.action(QStringLiteral("actionRedo"))->isEnabled());
    w.action(QStringLiteral("actionRedo"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
}

void TestViews::tempoBoxCommitsAndRevertsOnRejection()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    ValueBox* box = w.transport()->tempoBox();
    QCOMPARE(box->value(), bridge.tempo());

    emit box->committed(140.25);
    QCOMPARE(bridge.tempo(), 140.25);
    QCOMPARE(box->value(), 140.25);

    emit box->committed(5000.0);
    QCOMPARE(bridge.tempo(), 140.25);
    QCOMPARE(box->value(), 140.25);
    QVERIFY(w.statusBar()->currentMessage().contains(QStringLiteral("outside")));

    bridge.undo();
    QCOMPARE(box->value(), bridge.tempo());
    QVERIFY(!qFuzzyCompare(box->value(), 140.25));
}

void TestViews::viewSwitchButtonsAndToggle()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    QVERIFY(w.isSessionVisible());
    QVERIFY(w.transport()->sessionButton()->isChecked());
    w.transport()->arrangementButton()->click();
    QVERIFY(!w.isSessionVisible());
    QVERIFY(w.transport()->arrangementButton()->isChecked());
    QVERIFY(!w.transport()->sessionButton()->isChecked());
    w.toggleView();
    QVERIFY(w.isSessionVisible());
    QVERIFY(w.transport()->sessionButton()->isChecked());
    w.showArrangement();
    QVERIFY(!w.isSessionVisible());
    w.showSession();
    QVERIFY(w.isSessionVisible());
}

void TestViews::themeSwitchRepaintsWithNewTokens()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    bridge.addTrack();
    w.showArrangement();
    QVERIFY(m_themes.load(QStringLiteral("paper")));
    const QImage img = w.arrangementView()->viewport()->grab().toImage();
    const QRect lane = w.arrangementView()->laneRect(0);
    QVERIFY(!lane.isEmpty());
    QCOMPARE(img.pixelColor(lane.center()), m_themes.theme().color(QStringLiteral("arrangement.lane")));
    QCOMPARE(img.pixelColor(1, lane.center().y()), m_themes.theme().trackColor(0));
    QVERIFY(m_themes.load(QStringLiteral("nylon")));
}

void TestViews::slotAndLaneGeometryFollowMetrics()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    bridge.addTrack();
    bridge.addTrack();
    const nylon::Theme& t = m_themes.theme();
    const int sep = t.metricInt(QStringLiteral("separator"));
    const QRect a = w.sessionView()->slotRect(0, 0);
    const QRect b = w.sessionView()->slotRect(1, 0);
    const QRect c = w.sessionView()->slotRect(0, 1);
    QCOMPARE(a.width(), t.metricInt(QStringLiteral("session.slot.width")));
    QCOMPARE(a.height(), t.metricInt(QStringLiteral("session.slot.height")));
    QCOMPARE(b.x() - a.x(), a.width() + sep);
    QCOMPARE(c.y() - a.y(), a.height() + sep);
    QCOMPARE(w.sessionView()->sceneCount(), t.metricInt(QStringLiteral("session.scene.count")));

    const QRect l0 = w.arrangementView()->laneRect(0);
    const QRect l1 = w.arrangementView()->laneRect(1);
    QCOMPARE(l0.height(), t.metricInt(QStringLiteral("arrangement.lane.height")));
    QCOMPARE(l1.y() - l0.y(), l0.height() + sep);
    QCOMPARE(l0.x(), t.metricInt(QStringLiteral("arrangement.header.width")) + sep);
}

void TestViews::extremeMetricsStayWithinIntRange()
{
    // Write a user override that keeps every color but pushes the layout
    // metrics to the accepted maximum, then load it through the manager.
    QStandardPaths::setTestModeEnabled(true);
    const QString dir = ThemeManager::userThemeDirectory();
    QVERIFY(QDir().mkpath(dir));
    QFile src(QStringLiteral(":/themes/nylon.theme"));
    QVERIFY(src.open(QIODevice::ReadOnly | QIODevice::Text));
    QString text = QString::fromUtf8(src.readAll());
    const QStringList extreme{
        QStringLiteral("session.scene.count"), QStringLiteral("session.slot.width"),
        QStringLiteral("session.slot.height"), QStringLiteral("session.master.width"),
        QStringLiteral("arrangement.bars"), QStringLiteral("arrangement.pixels_per_bar"),
        QStringLiteral("arrangement.lane.height"), QStringLiteral("arrangement.header.width"),
        QStringLiteral("arrangement.ruler.height"), QStringLiteral("separator"),
    };
    for (const QString& key : extreme) {
        const QRegularExpression re(QStringLiteral("^metric\\.%1 = .*$").arg(QRegularExpression::escape(key)),
            QRegularExpression::MultilineOption);
        QVERIFY2(text.contains(re), qPrintable(key));
        text.replace(re, QStringLiteral("metric.%1 = 100000").arg(key));
    }
    text.replace(QStringLiteral("name = Nylon"), QStringLiteral("name = Extreme"));
    QFile out(dir + QStringLiteral("/extreme.theme"));
    QVERIFY(out.open(QIODevice::WriteOnly | QIODevice::Truncate | QIODevice::Text));
    out.write(text.toUtf8());
    out.close();

    ThemeManager themes;
    QVERIFY2(themes.load(QStringLiteral("extreme")), qPrintable(themes.lastErrors().join(QStringLiteral("; "))));
    QVERIFY(themes.isUserOverride());
    ProjectBridge bridge;
    MainWindow w(&bridge, &themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    for (int i = 0; i < 40; ++i) {
        QVERIFY(bridge.addTrack());
    }

    SessionView* session = w.sessionView();
    ArrangementView* arrangement = w.arrangementView();
    const nylon::Theme& t = themes.theme();
    const qint64 sep = t.metricInt(QStringLiteral("separator"));
    const qint64 slotPitch = t.metricInt(QStringLiteral("session.slot.width")) + sep;
    const qint64 scenePitch = t.metricInt(QStringLiteral("session.slot.height")) + sep;
    QCOMPARE(sep, qint64(100000));
    // Requested 100000 scenes at a 200000 px pitch cannot fit; the laid-out
    // count is capped and the scroll range clamped, never negative or wrapped.
    QVERIFY(session->sceneCount() >= 1);
    QVERIFY(qint64(session->sceneCount()) * scenePitch <= layout::kMaxExtent);
    QVERIFY(session->columnCount() >= 1);
    QVERIFY(session->columnCount() <= 40);
    QVERIFY(session->verticalScrollBar()->maximum() >= 0);
    QVERIFY(session->verticalScrollBar()->maximum() <= layout::kMaxExtent);
    QVERIFY(session->horizontalScrollBar()->maximum() >= 0);
    QVERIFY(session->horizontalScrollBar()->maximum() <= layout::kMaxExtent);
    QVERIFY(arrangement->barCount() >= 1);
    QVERIFY(arrangement->laneCount() >= 1);
    QVERIFY(arrangement->verticalScrollBar()->maximum() <= layout::kMaxExtent);
    QVERIFY(arrangement->horizontalScrollBar()->maximum() <= layout::kMaxExtent);

    // Geometry for the last laid-out cells is representable and consistent.
    const int lastCol = session->columnCount() - 1;
    const QRect a = session->slotRect(lastCol, 0);
    QVERIFY(!a.isEmpty());
    QCOMPARE(qint64(a.x()), qint64(lastCol) * slotPitch);
    QVERIFY(session->slotRect(session->columnCount(), 0).isEmpty());
    QVERIFY(session->slotRect(0, session->sceneCount()).isEmpty());
    // A 200000 px header leaves no lane body inside the viewport, so the
    // rect is zero-width; its vertical placement must still be valid.
    const QRect l = arrangement->laneRect(arrangement->laneCount() - 1);
    QCOMPARE(l.height(), t.metricInt(QStringLiteral("arrangement.lane.height")));
    QCOMPARE(l.x(), t.metricInt(QStringLiteral("arrangement.header.width")) + static_cast<int>(sep));
    QVERIFY(layout::fitsCoordinate(l.y()));
    QCOMPARE(arrangement->laneRect(arrangement->laneCount()).height(), 0);

    // Scrolled to the far end, painting must still complete and stay
    // within the visible window.
    session->horizontalScrollBar()->setValue(session->horizontalScrollBar()->maximum());
    session->verticalScrollBar()->setValue(session->verticalScrollBar()->maximum());
    arrangement->horizontalScrollBar()->setValue(arrangement->horizontalScrollBar()->maximum());
    arrangement->verticalScrollBar()->setValue(arrangement->verticalScrollBar()->maximum());
    QElapsedTimer timer;
    timer.start();
    QVERIFY(!session->viewport()->grab().toImage().isNull());
    w.showArrangement();
    QVERIFY(!arrangement->viewport()->grab().toImage().isNull());
    // Two paints of a 100000-scene, 100000-bar layout: visible-range
    // iteration keeps this in the tens of milliseconds, not minutes.
    QVERIFY2(timer.elapsed() < 5000, qPrintable(QString::number(timer.elapsed())));

    QVERIFY(QFile::remove(dir + QStringLiteral("/extreme.theme")));
    QStandardPaths::setTestModeEnabled(false);
}

void TestViews::transportSpacingFollowsTokens()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    const nylon::Theme& t = m_themes.theme();
    QCOMPARE(w.transport()->height(), t.metricInt(QStringLiteral("transport.height")) + 8);
    QCOMPARE(w.transport()->tempoBox()->width(), t.metricInt(QStringLiteral("transport.tempo.width")));
    const int side = t.metricInt(QStringLiteral("transport.button.size")) + 6;
    QCOMPARE(w.transport()->playButton()->width(), side);
    QVERIFY(!w.transport()->playButton()->isEnabled());
    QVERIFY(!w.transport()->isTransportAvailable());

    // The time signature boxes read and write the core.
    QCOMPARE(w.transport()->numeratorBox()->value(), 4.0);
    emit w.transport()->numeratorBox()->committed(3.0);
    QCOMPARE(bridge.timeSignatureNumerator(), 3);
    emit w.transport()->denominatorBox()->committed(5.0);
    QCOMPARE(bridge.timeSignatureDenominator(), 4);
    QCOMPARE(w.transport()->denominatorBox()->value(), 4.0);
    QVERIFY(w.statusBar()->currentMessage().contains(QStringLiteral("not supported")));
}

void TestViews::startScreenThenWorkspace()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    QVERIFY(w.isStartScreenVisible());
    QVERIFY(w.startScreen()->newButton()->isEnabled());
    QVERIFY(w.startScreen()->openButton()->isEnabled());
    // The launcher is a compact centered card, not a canvas-wide layout.
    const QRect card = w.startScreen()->cardRect();
    QVERIFY(card.width() <= 700);
    QVERIFY(card.height() < w.startScreen()->height() / 2);
    QVERIFY(qAbs(card.center().x() - w.startScreen()->width() / 2) < 4);
    QVERIFY(card.contains(w.startScreen()->newButton()->geometry().translated(card.topLeft())));
    bridge.addTrack();
    QCOMPARE(bridge.trackCount(), 1ull);
    w.startScreen()->newButton()->click();
    QVERIFY(!w.isStartScreenVisible());
    // New Project starts from an empty core project.
    QCOMPARE(bridge.trackCount(), 0ull);
    QVERIFY(!bridge.undo());
    w.action(QStringLiteral("actionClose"))->trigger();
    QVERIFY(w.isStartScreenVisible());
}

void TestViews::menusAreInWindowAndComplete()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    QMenuBar* bar = w.titleBar()->menuBar();
    QVERIFY(!bar->isNativeMenuBar());
    QStringList titles;
    for (QAction* a : bar->actions()) {
        titles.append(a->text().remove(QLatin1Char('&')));
    }
    QCOMPARE(titles, (QStringList{QStringLiteral("File"), QStringLiteral("Edit"), QStringLiteral("Create"),
        QStringLiteral("View"), QStringLiteral("Transport"), QStringLiteral("Help")}));
    for (const char* name : {"actionNew", "actionOpen", "actionSave", "actionPreferences", "actionQuit", "actionUndo",
             "actionRedo", "actionAddTrack", "actionAddMidiTrack", "actionToggleBrowser", "actionToggleDetail",
             "actionToggleMixer", "actionSession", "actionArrangement", "actionFullScreen", "actionPlay",
             "actionStop", "actionRecord", "actionLoop", "actionAbout"}) {
        QVERIFY2(w.action(QLatin1String(name)), name);
    }
    // Everything the core cannot do yet is disabled and says why.
    for (const char* name : {"actionPlay", "actionCut"}) {
        QAction* a = w.action(QLatin1String(name));
        QVERIFY2(!a->isEnabled(), name);
        QVERIFY2(!a->statusTip().isEmpty(), name);
    }
    QVERIFY(w.action(QStringLiteral("actionNew"))->isEnabled());
    QVERIFY(w.action(QStringLiteral("actionOpen"))->isEnabled());
    QVERIFY(w.action(QStringLiteral("actionSave"))->isEnabled());
    QVERIFY(w.action(QStringLiteral("actionAddTrack"))->isEnabled());
    QVERIFY(w.action(QStringLiteral("actionAddMidiTrack"))->isEnabled());
    QVERIFY(!w.action(QStringLiteral("actionUndo"))->isEnabled());
    w.newProject();
    w.action(QStringLiteral("actionAddMidiTrack"))->trigger();
    QCOMPARE(bridge.trackKind(0), ProjectBridge::TrackKind::Midi);
    QVERIFY(w.action(QStringLiteral("actionUndo"))->isEnabled());
    QVERIFY(!w.action(QStringLiteral("actionRedo"))->isEnabled());
}

void TestViews::selectionFlowsBetweenGridMixerAndDetail()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    QCOMPARE(w.detail()->selectedTrack(), -1);
    QCOMPARE(w.mixer()->stripCount(), 0);
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    QCOMPARE(w.mixer()->stripCount(), 3);
    // Inserting selects the new track everywhere.
    QCOMPARE(w.sessionView()->selectedTrack(), 2);
    QCOMPARE(w.mixer()->selectedTrack(), 2);
    QCOMPARE(w.detail()->selectedTrack(), 2);
    QCOMPARE(w.detail()->headerText(), bridge.trackName(2));
    QCOMPARE(w.mixer()->strip(2)->name(), bridge.trackName(2));
    QVERIFY(!bridge.trackName(2).isEmpty());

    // Clicking a slot in the grid selects that column and shows the clip page.
    const QRect slot = w.sessionView()->slotRect(0, 1);
    QTest::mouseClick(w.sessionView()->viewport(), Qt::LeftButton, Qt::NoModifier, slot.center());
    QCOMPARE(w.sessionView()->selectedTrack(), 0);
    QCOMPARE(w.mixer()->selectedTrack(), 0);
    QCOMPARE(w.detail()->selectedTrack(), 0);
    QCOMPARE(w.detail()->page(), DetailPanel::Page::Clip);
    QVERIFY(w.mixer()->strip(0)->isSelected());
    QVERIFY(!w.mixer()->strip(2)->isSelected());

    // Clicking a strip selects it.
    QTest::mouseClick(w.mixer()->strip(1), Qt::LeftButton, Qt::NoModifier, QPoint(4, 4));
    QCOMPARE(w.sessionView()->selectedTrack(), 1);
    QCOMPARE(w.detail()->selectedTrack(), 1);

    // Undoing the last insert drops the selection to a live track.
    w.selectTrack(2);
    w.action(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(w.mixer()->stripCount(), 2);
    QVERIFY(w.detail()->selectedTrack() < 2);

    // Mixer controls write to the core and one gesture is one undo step.
    MixerStrip* strip = w.mixer()->strip(0);
    QVERIFY(strip->isInteractive());
    QVERIFY(strip->fader()->isEnabled());
    QVERIFY(w.mixer()->masterStrip());
    strip->soloButton()->click();
    QVERIFY(bridge.trackSolo(0));
    strip->armButton()->click();
    QVERIFY(bridge.trackArmed(0));
    strip->activator()->click();
    QVERIFY(bridge.trackMuted(0));
    strip->activator()->click();
    QVERIFY(!bridge.trackMuted(0));
    emit strip->fader()->dragStarted();
    strip->fader()->setValue(-12.0);
    strip->fader()->setValue(-9.0);
    emit strip->fader()->dragFinished();
    QCOMPARE(bridge.trackVolumeDb(0), -9.0);
    w.action(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(bridge.trackVolumeDb(0), 0.0);
    QCOMPARE(strip->fader()->value(), 0.0);
    strip->fader()->setValue(strip->fader()->minimum());
    emit strip->fader()->dragFinished();
    QVERIFY(std::isinf(bridge.trackVolumeDb(0)));
    emit strip->pan()->dragStarted();
    strip->pan()->setValue(0.5);
    emit strip->pan()->dragFinished();
    QCOMPARE(bridge.trackPan(0), 0.5);

    // Renaming through the core shows up in the strip, grid header, and detail.
    QVERIFY(bridge.setTrackName(0, QStringLiteral("Bass")));
    QCOMPARE(strip->name(), QStringLiteral("Bass"));
    w.selectTrack(0);
    QCOMPARE(w.detail()->headerText(), QStringLiteral("Bass"));
    w.action(QStringLiteral("actionDelete"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
    QCOMPARE(w.mixer()->stripCount(), 1);
    QVERIFY(w.action(QStringLiteral("actionUndo"))->isEnabled());
    w.action(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(bridge.trackCount(), 2ull);
    QVERIFY(w.action(QStringLiteral("actionRedo"))->isEnabled());
}

void TestViews::browserShowsLibraryCategories()
{
    QStandardPaths::setTestModeEnabled(true);
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    BrowserPanel* b = w.browser();
    QCOMPARE(b->categoryList()->count(), BrowserPanel::categories().size());
    QCOMPARE(b->currentCategory(), QStringLiteral("Sounds"));
    QVERIFY(QDir(b->currentFolder()).exists());
    b->selectCategory(QStringLiteral("Samples"));
    QCOMPARE(b->currentCategory(), QStringLiteral("Samples"));
    QVERIFY(b->currentFolder().endsWith(QStringLiteral("/Samples")));
    QVERIFY(QDir(b->currentFolder()).exists());
    // A file dropped into the folder shows up; searching filters it.
    QFile probe(b->currentFolder() + QStringLiteral("/kick.wav"));
    QVERIFY(probe.open(QIODevice::WriteOnly));
    probe.write("RIFF");
    probe.close();
    b->reload();
    QTRY_COMPARE_WITH_TIMEOUT(b->visibleEntryCount(), 1, 3000);
    QVERIFY(!b->isShowingEmptyState());
    b->searchField()->setText(QStringLiteral("snare"));
    QTRY_VERIFY_WITH_TIMEOUT(b->isShowingEmptyState(), 3000);
    b->searchField()->clear();
    QTRY_VERIFY_WITH_TIMEOUT(!b->isShowingEmptyState(), 3000);
    QVERIFY(QFile::remove(probe.fileName()));
    w.action(QStringLiteral("actionToggleBrowser"))->toggle();
    QVERIFY(!b->isVisible());
    w.action(QStringLiteral("actionToggleBrowser"))->toggle();
    QVERIFY(b->isVisible());
    QStandardPaths::setTestModeEnabled(false);
}

void TestViews::framelessWindowWithTitleBar()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    QVERIFY(w.windowFlags() & Qt::FramelessWindowHint);
    QVERIFY(w.testAttribute(Qt::WA_TranslucentBackground));
    TitleBar* bar = w.titleBar();
    QVERIFY(bar);
    QCOMPARE(bar->height(), m_themes.theme().metricInt(QStringLiteral("titlebar.height")));
    QVERIFY(bar->menuBar());
    QVERIFY(bar->menuBar()->isVisibleTo(bar));
    QVERIFY(bar->menuBar()->actions().size() >= 6);
    QCOMPARE(bar->title(), QStringLiteral("Nylon"));
    w.newProject();
    QVERIFY(bar->title().startsWith(QStringLiteral("Untitled")));
    QVERIFY(!bar->closeRect().isEmpty());
    QVERIFY(bar->closeRect().x() < bar->minimizeRect().x());
    QVERIFY(bar->minimizeRect().x() < bar->zoomRect().x());

    QSignalSpy zoom(bar, &TitleBar::zoomRequested);
    QSignalSpy minimize(bar, &TitleBar::minimizeRequested);
    QSignalSpy close(bar, &TitleBar::closeRequested);
    QTest::mouseClick(bar, Qt::LeftButton, Qt::NoModifier, bar->zoomRect().center());
    QCOMPARE(zoom.count(), 1);
    QTest::mouseClick(bar, Qt::LeftButton, Qt::NoModifier, bar->minimizeRect().center());
    QCOMPARE(minimize.count(), 1);
    QTest::mouseClick(bar, Qt::LeftButton, Qt::NoModifier, bar->closeRect().center());
    QCOMPARE(close.count(), 1);
    // Double-clicking the empty part of the bar toggles maximize.
    QTest::mouseDClick(bar, Qt::LeftButton, Qt::NoModifier, QPoint(bar->width() / 2, bar->height() / 2));
    QCOMPARE(zoom.count(), 2);

    // The window paints rounded: the very corner pixel stays transparent
    // while a pixel just inside is the background.
    const QImage img = w.grab().toImage();
    QVERIFY(img.pixelColor(0, 0).alpha() < 255 || img.pixelColor(0, 0) != m_themes.theme().color(QStringLiteral("titlebar.background")));
    // Just inside the outline, above the window controls, the title band shows.
    const int r = m_themes.theme().metricInt(QStringLiteral("radius"));
    QCOMPARE(img.pixelColor(r + 4, 3), m_themes.theme().color(QStringLiteral("titlebar.background")));
}

void TestViews::saveOpenAndRecentThroughTheWindow()
{
    QStandardPaths::setTestModeEnabled(true);
    StartScreen::clearRecentProjects();
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString bundle = dir.path() + QStringLiteral("/Demo.nylon");

    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    w.newProject();
    QVERIFY(w.titleBar()->title().startsWith(QStringLiteral("Untitled")));
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    QVERIFY(bridge.setTrackName(0, QStringLiteral("Drums")));
    QVERIFY(w.saveProjectTo(bundle));
    QCOMPARE(bridge.bundlePath(), bundle);
    QCOMPARE(w.titleBar()->title(), QStringLiteral("Demo - Nylon"));
    QVERIFY(w.statusBar()->currentMessage().startsWith(QStringLiteral("Saved")));
    QCOMPARE(StartScreen::recentProjects(), QStringList{bundle});

    // Save without a dialog once the bundle path is known.
    w.action(QStringLiteral("actionAddTrack"))->trigger();
    QVERIFY(w.saveProject());
    QCOMPARE(bridge.trackCount(), 2ull);

    // A fresh project, then reopen through the recent list on the start screen.
    w.newProject();
    QCOMPARE(bridge.trackCount(), 0ull);
    w.action(QStringLiteral("actionClose"))->trigger();
    QVERIFY(w.isStartScreenVisible());
    QCOMPARE(w.startScreen()->recentList()->count(), 1);
    QVERIFY(w.startScreen()->recentList()->isEnabled());
    emit w.startScreen()->recentProjectRequested(bundle);
    QVERIFY(!w.isStartScreenVisible());
    QCOMPARE(bridge.trackCount(), 2ull);
    QCOMPARE(bridge.trackName(0), QStringLiteral("Drums"));
    QCOMPARE(w.mixer()->stripCount(), 2);
    QCOMPARE(w.titleBar()->title(), QStringLiteral("Demo - Nylon"));

    // The recent menu lists the bundle ahead of the clear entry.
    QMenu* recentMenu = w.findChild<QMenu*>();
    Q_UNUSED(recentMenu);
    QAction* clear = w.action(QStringLiteral("actionClearRecent"));
    QVERIFY(clear->isEnabled());
    QMenu* menu = qobject_cast<QMenu*>(clear->parent());
    QVERIFY(menu);
    QCOMPARE(menu->actions().first()->text(), QStringLiteral("Demo"));

    // Opening something that is not a bundle keeps the current project.
    QVERIFY(!w.openProjectAt(dir.path() + QStringLiteral("/nope.nylon")));
    QCOMPARE(bridge.trackCount(), 2ull);
    QVERIFY(w.statusBar()->currentMessage().startsWith(QStringLiteral("Could not open")));

    clear->trigger();
    QVERIFY(StartScreen::recentProjects().isEmpty());
    QVERIFY(!clear->isEnabled());
    QStandardPaths::setTestModeEnabled(false);
}

QTEST_MAIN(TestViews)
#include "test_views.moc"
