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
#include <QDir>
#include <QFile>
#include <QScrollBar>
#include <QStandardPaths>
#include <QtTest>

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
    const nylon::Theme& t = m_themes.theme();
    QCOMPARE(w.transport()->height(), t.metricInt(QStringLiteral("transport.height")));
    QCOMPARE(w.transport()->tempoBox()->width(), t.metricInt(QStringLiteral("transport.tempo.width")));
    const int gap = t.metricInt(QStringLiteral("transport.spacing"));
    QCOMPARE(w.transport()->addTrackButton()->x() - (w.transport()->tempoBox()->x() + w.transport()->tempoBox()->width()), gap);
}

QTEST_MAIN(TestViews)
#include "test_views.moc"
