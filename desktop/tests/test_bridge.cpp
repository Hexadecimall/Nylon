#include "ProjectBridge.h"

#include "nylon.h"

#include <QtTest>
#include <cmath>

using nylon::ProjectBridge;

class TestBridge : public QObject {
    Q_OBJECT
private slots:
    void nullHandleIsRejectedByEveryFunction();
    void newProjectHasDefaultTempoAndNoTracks();
    void setTempoAcceptsRangeAndRejectsOutside();
    void addTrackUndoRedo();
    void redoStackClearsAfterNewEdit();
    void changedEmittedOnlyOnSuccess();
};

void TestBridge::nullHandleIsRejectedByEveryFunction()
{
    nylon_project_free(nullptr);
    QCOMPARE(nylon_project_tempo(nullptr), 0.0);
    QCOMPARE(nylon_project_set_tempo(nullptr, 120.0), 0);
    QCOMPARE(nylon_project_add_track(nullptr), 0);
    QCOMPARE(nylon_project_track_count(nullptr), 0ull);
    QCOMPARE(nylon_project_undo(nullptr), 0);
    QCOMPARE(nylon_project_redo(nullptr), 0);
}

void TestBridge::newProjectHasDefaultTempoAndNoTracks()
{
    ProjectBridge b;
    QVERIFY(b.isValid());
    QVERIFY(b.tempo() >= 20.0 && b.tempo() <= 999.0);
    QCOMPARE(b.trackCount(), 0ull);
}

void TestBridge::setTempoAcceptsRangeAndRejectsOutside()
{
    ProjectBridge b;
    QVERIFY(b.setTempo(128.5));
    QCOMPARE(b.tempo(), 128.5);
    QVERIFY(b.setTempo(20.0));
    QCOMPARE(b.tempo(), 20.0);
    QVERIFY(b.setTempo(999.0));
    QCOMPARE(b.tempo(), 999.0);
    QVERIFY(!b.setTempo(0.0));
    QCOMPARE(b.tempo(), 999.0);
    QVERIFY(!b.setTempo(-1.0));
    QVERIFY(!b.setTempo(1000.0));
    QVERIFY(!b.setTempo(std::nan("")));
    QVERIFY(!b.setTempo(std::numeric_limits<double>::infinity()));
    QCOMPARE(b.tempo(), 999.0);
}

void TestBridge::addTrackUndoRedo()
{
    ProjectBridge b;
    QVERIFY(!b.undo());
    QVERIFY(!b.redo());
    QVERIFY(b.addTrack());
    QVERIFY(b.addTrack());
    QCOMPARE(b.trackCount(), 2ull);
    QVERIFY(b.undo());
    QCOMPARE(b.trackCount(), 1ull);
    QVERIFY(b.undo());
    QCOMPARE(b.trackCount(), 0ull);
    QVERIFY(!b.undo());
    QVERIFY(b.redo());
    QVERIFY(b.redo());
    QCOMPARE(b.trackCount(), 2ull);
    QVERIFY(!b.redo());
}

void TestBridge::redoStackClearsAfterNewEdit()
{
    ProjectBridge b;
    QVERIFY(b.setTempo(100.0));
    QVERIFY(b.setTempo(110.0));
    QVERIFY(b.undo());
    QCOMPARE(b.tempo(), 100.0);
    QVERIFY(b.addTrack());
    QVERIFY(!b.redo());
    QCOMPARE(b.tempo(), 100.0);
    QCOMPARE(b.trackCount(), 1ull);
}

void TestBridge::changedEmittedOnlyOnSuccess()
{
    ProjectBridge b;
    QSignalSpy spy(&b, &ProjectBridge::changed);
    QVERIFY(!b.setTempo(0.0));
    QCOMPARE(spy.count(), 0);
    QVERIFY(!b.undo());
    QCOMPARE(spy.count(), 0);
    QVERIFY(b.setTempo(90.0));
    QCOMPARE(spy.count(), 1);
    QVERIFY(b.addTrack());
    QCOMPARE(spy.count(), 2);
    QVERIFY(b.undo());
    QCOMPARE(spy.count(), 3);
    QVERIFY(b.redo());
    QCOMPARE(spy.count(), 4);
}

QTEST_GUILESS_MAIN(TestBridge)
#include "test_bridge.moc"
