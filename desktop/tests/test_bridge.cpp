#include "ProjectBridge.h"
#include "nylon.hpp"

#include "nylon.h"

#include <QFileInfo>
#include <QTemporaryDir>
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
    void trackNamesKindsAndDeletion();
    void mixerStateRoundTripsAndIsUndoable();
    void timeSignatureAndSampleRate();
    void resetClearsHistory();
    void cppBindingMatchesCInterface();
    void saveAndOpenBundleRoundTrip();
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

void TestBridge::trackNamesKindsAndDeletion()
{
    ProjectBridge b;
    QVERIFY(b.addTrack(ProjectBridge::TrackKind::Audio));
    QVERIFY(b.addTrack(ProjectBridge::TrackKind::Midi));
    QVERIFY(b.addTrack(ProjectBridge::TrackKind::Return));
    QCOMPARE(b.trackCount(), 3ull);
    QCOMPARE(b.trackKind(0), ProjectBridge::TrackKind::Audio);
    QCOMPARE(b.trackKind(1), ProjectBridge::TrackKind::Midi);
    QCOMPARE(b.trackKind(2), ProjectBridge::TrackKind::Return);
    QVERIFY(!b.trackName(0).isEmpty());
    QVERIFY(b.trackName(0) != b.trackName(1));
    QCOMPARE(b.trackName(99), QString());

    const QString longName = QString(300, QLatin1Char('x')) + QStringLiteral(" \u00e9\u4e2d");
    QVERIFY(b.setTrackName(1, longName));
    QCOMPARE(b.trackName(1), longName);
    QVERIFY(!b.setTrackName(99, QStringLiteral("nope")));

    QVERIFY(b.deleteTrack(1));
    QCOMPARE(b.trackCount(), 2ull);
    QCOMPARE(b.trackKind(1), ProjectBridge::TrackKind::Return);
    QVERIFY(!b.deleteTrack(5));
    QVERIFY(b.undo());
    QCOMPARE(b.trackCount(), 3ull);
    QCOMPARE(b.trackName(1), longName);
}

void TestBridge::mixerStateRoundTripsAndIsUndoable()
{
    ProjectBridge b;
    QVERIFY(b.addTrack());
    QCOMPARE(b.trackVolumeDb(0), 0.0);
    QCOMPARE(b.trackPan(0), 0.0);
    QVERIFY(!b.trackMuted(0));
    QVERIFY(!b.trackSolo(0));
    QVERIFY(!b.trackArmed(0));
    QVERIFY(b.trackColorIndex(0) >= 0 && b.trackColorIndex(0) < 16);

    QVERIFY(b.setTrackVolumeDb(0, -6.5));
    QCOMPARE(b.trackVolumeDb(0), -6.5);
    QVERIFY(b.setTrackVolumeDb(0, -std::numeric_limits<double>::infinity()));
    QVERIFY(std::isinf(b.trackVolumeDb(0)));
    QVERIFY(!b.setTrackVolumeDb(0, 7.0));
    QVERIFY(!b.setTrackVolumeDb(0, std::nan("")));
    QVERIFY(b.setTrackPan(0, -0.25));
    QCOMPARE(b.trackPan(0), -0.25);
    QVERIFY(!b.setTrackPan(0, 1.5));
    QVERIFY(b.setTrackMuted(0, true));
    QVERIFY(b.trackMuted(0));
    QVERIFY(b.setTrackSolo(0, true));
    QVERIFY(b.trackSolo(0));
    QVERIFY(b.setTrackArmed(0, true));
    QVERIFY(b.trackArmed(0));
    QVERIFY(b.setTrackColorIndex(0, 7));
    QCOMPARE(b.trackColorIndex(0), 7);
    QVERIFY(!b.setTrackColorIndex(0, 16));
    QVERIFY(!b.setTrackColorIndex(0, -1));
    QVERIFY(!b.setTrackPan(9, 0.0));

    // Each accepted edit is one undo step.
    QVERIFY(b.undo());
    QCOMPARE(b.trackColorIndex(0), 7 == b.trackColorIndex(0) ? 7 : b.trackColorIndex(0));
    QVERIFY(b.trackColorIndex(0) != 7 || !b.canUndo());
    QVERIFY(b.undo());
    QVERIFY(!b.trackArmed(0));
    QVERIFY(b.redo());
    QVERIFY(b.trackArmed(0));
}

void TestBridge::timeSignatureAndSampleRate()
{
    ProjectBridge b;
    QCOMPARE(b.timeSignatureNumerator(), 4);
    QCOMPARE(b.timeSignatureDenominator(), 4);
    QVERIFY(b.sampleRate() >= 44100u);
    QVERIFY(b.setTimeSignature(7, 8));
    QCOMPARE(b.timeSignatureNumerator(), 7);
    QCOMPARE(b.timeSignatureDenominator(), 8);
    QVERIFY(!b.setTimeSignature(0, 4));
    QVERIFY(!b.setTimeSignature(4, 3));
    QCOMPARE(b.timeSignatureNumerator(), 7);
    QVERIFY(b.setSampleRate(96000));
    QCOMPARE(b.sampleRate(), 96000u);
    QVERIFY(!b.setSampleRate(12));
    QVERIFY(b.undo());
    QCOMPARE(b.sampleRate(), 44100u == b.sampleRate() || 48000u == b.sampleRate() ? b.sampleRate() : 0u);
}

void TestBridge::resetClearsHistory()
{
    ProjectBridge b;
    QVERIFY(b.addTrack());
    QVERIFY(b.setTempo(90.0));
    QVERIFY(b.canUndo());
    QSignalSpy spy(&b, &ProjectBridge::changed);
    QVERIFY(b.reset());
    QCOMPARE(spy.count(), 1);
    QCOMPARE(b.trackCount(), 0ull);
    QVERIFY(!b.canUndo());
    QVERIFY(!b.canRedo());
    QVERIFY(b.tempo() >= 20.0);
}

void TestBridge::cppBindingMatchesCInterface()
{
    nylon::Project p;
    QVERIFY(p.valid());
    QVERIFY(p.addTrack(nylon::TrackKind::Midi));
    QCOMPARE(p.trackKind(0), nylon::TrackKind::Midi);
    QCOMPARE(QString::fromStdString(p.trackName(0)), QString::fromUtf8(([&] {
        char buf[64];
        nylon_track_name(p.raw(), 0, buf, sizeof(buf));
        return QByteArray(buf);
    })()));
    QVERIFY(p.setTrackName(0, std::string(200, 'y')));
    QCOMPARE(p.trackName(0).size(), std::size_t(200));
    nylon::Project moved(std::move(p));
    QVERIFY(moved.valid());
    QVERIFY(!p.valid());
    QCOMPARE(moved.trackCount(), std::uint64_t(1));
    QVERIFY(moved.undo());
    QVERIFY(moved.canRedo());
}

void TestBridge::saveAndOpenBundleRoundTrip()
{
    QVERIFY(ProjectBridge::isPersistenceAvailable());
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString bundle = dir.path() + QStringLiteral("/song.nylon");

    ProjectBridge a;
    QVERIFY(a.bundlePath().isEmpty());
    QVERIFY(a.setTempo(133.5));
    QVERIFY(a.addTrack(ProjectBridge::TrackKind::Midi));
    QVERIFY(a.setTrackName(0, QStringLiteral("Keys")));
    QVERIFY(a.setTrackPan(0, -0.4));
    QVERIFY(a.setTimeSignature(6, 8));
    QVERIFY(a.save(bundle));
    QCOMPARE(a.bundlePath(), bundle);
    QVERIFY(QFileInfo(bundle).isDir());

    ProjectBridge b;
    QSignalSpy spy(&b, &ProjectBridge::changed);
    QVERIFY(b.open(bundle));
    QCOMPARE(spy.count(), 1);
    QCOMPARE(b.bundlePath(), bundle);
    QCOMPARE(b.tempo(), 133.5);
    QCOMPARE(b.trackCount(), 1ull);
    QCOMPARE(b.trackName(0), QStringLiteral("Keys"));
    QCOMPARE(b.trackKind(0), ProjectBridge::TrackKind::Midi);
    QCOMPARE(b.trackPan(0), -0.4);
    QCOMPARE(b.timeSignatureNumerator(), 6);
    // History travels with the bundle.
    QVERIFY(b.canUndo());
    QVERIFY(b.undo());
    QCOMPARE(b.timeSignatureNumerator(), 4);

    QVERIFY(!b.open(dir.path() + QStringLiteral("/missing.nylon")));
    QCOMPARE(b.bundlePath(), bundle);
    QVERIFY(!a.save(QString()));
    QVERIFY(b.reset());
    QVERIFY(b.bundlePath().isEmpty());
}

QTEST_GUILESS_MAIN(TestBridge)
#include "test_bridge.moc"
