#include "ArrangementView.h"
#include "MainWindow.h"
#include "ProjectBridge.h"
#include "SessionView.h"
#include "ThemeManager.h"
#include "TransportBar.h"

#include <QAction>
#include <QDoubleSpinBox>
#include <QPushButton>
#include <QStatusBar>
#include <QToolButton>
#include <QtTest>

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
    QVERIFY(w.sessionView()->isShowingEmptyState());
    QVERIFY(w.arrangementView()->isShowingEmptyState());
    QCOMPARE(w.sessionView()->columnCount(), 0);
    QCOMPARE(w.arrangementView()->laneCount(), 0);
    QVERIFY(w.sessionView()->slotRect(0, 0).isEmpty());
    QVERIFY(w.arrangementView()->laneRect(0).isEmpty());
    // Painting the empty state must not fail.
    const QImage img = w.sessionView()->viewport()->grab().toImage();
    QVERIFY(!img.isNull());
    QCOMPARE(img.pixelColor(2, img.height() - 3), m_themes.theme().color(QStringLiteral("background")));
}

void TestViews::addTrackButtonGrowsBothViews()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    QTest::mouseClick(w.transport()->addTrackButton(), Qt::LeftButton);
    QTest::mouseClick(w.transport()->addTrackButton(), Qt::LeftButton);
    QTest::mouseClick(w.transport()->addTrackButton(), Qt::LeftButton);
    QCOMPARE(bridge.trackCount(), 3ull);
    QCOMPARE(w.sessionView()->columnCount(), 3);
    QCOMPARE(w.arrangementView()->laneCount(), 3);
    QVERIFY(!w.sessionView()->isShowingEmptyState());
    QVERIFY(!w.arrangementView()->isShowingEmptyState());

    // The third column's header carries the third track color.
    const QRect slot = w.sessionView()->slotRect(2, 0);
    QVERIFY(!slot.isEmpty());
    const QImage img = w.sessionView()->viewport()->grab().toImage();
    QCOMPARE(img.pixelColor(slot.center().x(), 0), m_themes.theme().trackColor(2));
    QCOMPARE(img.pixelColor(slot.center()), m_themes.theme().color(QStringLiteral("session.slot")));
}

void TestViews::undoRedoFromButtonsAndActions()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    QTest::mouseClick(w.transport()->undoButton(), Qt::LeftButton);
    QCOMPARE(w.statusBar()->currentMessage(), QStringLiteral("Nothing to undo."));

    w.findChild<QAction*>(QStringLiteral("actionAddTrack"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
    w.findChild<QAction*>(QStringLiteral("actionUndo"))->trigger();
    QCOMPARE(bridge.trackCount(), 0ull);
    QCOMPARE(w.sessionView()->columnCount(), 0);
    w.findChild<QAction*>(QStringLiteral("actionRedo"))->trigger();
    QCOMPARE(bridge.trackCount(), 1ull);
    QTest::mouseClick(w.transport()->undoButton(), Qt::LeftButton);
    QCOMPARE(bridge.trackCount(), 0ull);
    QTest::mouseClick(w.transport()->redoButton(), Qt::LeftButton);
    QCOMPARE(bridge.trackCount(), 1ull);
    QTest::mouseClick(w.transport()->redoButton(), Qt::LeftButton);
    QCOMPARE(w.statusBar()->currentMessage(), QStringLiteral("Nothing to redo."));
}

void TestViews::tempoBoxCommitsAndRevertsOnRejection()
{
    ProjectBridge bridge;
    MainWindow w(&bridge, &m_themes);
    w.show();
    QVERIFY(QTest::qWaitForWindowExposed(&w));
    QDoubleSpinBox* box = w.transport()->tempoBox();
    QCOMPARE(box->value(), bridge.tempo());

    box->setValue(140.25);
    emit box->editingFinished();
    QCOMPARE(bridge.tempo(), 140.25);

    box->setValue(5000.0);
    emit box->editingFinished();
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
    QVERIFY(w.isSessionVisible());
    QVERIFY(w.transport()->sessionButton()->isChecked());
    QTest::mouseClick(w.transport()->arrangementButton(), Qt::LeftButton);
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
    bridge.addTrack();
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

QTEST_MAIN(TestViews)
#include "test_views.moc"
