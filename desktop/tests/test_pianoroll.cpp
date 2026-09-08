#include "PianoRoll.h"
#include "ProjectBridge.h"
#include "ThemeManager.h"

#include <QScrollBar>
#include <QWheelEvent>
#include <QtTest>

using namespace nylon;

class TestPianoRoll : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void emptyStateWithoutClip();
    void geometryRoundTrips();
    void doubleClickAddsAndRemovesNotes();
    void dragMovesNoteQuantized();
    void deleteKeyRemovesSelection();
    void refreshDropsStaleSelection();
    void resizeGripChangesLength();
    void velocityLaneDragChangesVelocity();
    void controlWheelZooms();

private:
    ThemeManager m_themes;
    void prepare(ProjectBridge& bridge)
    {
        QVERIFY(bridge.addTrack(ProjectBridge::TrackKind::Midi));
        QVERIFY(bridge.sceneCount() >= 1 || bridge.createScene(QStringLiteral("Scene 1")));
        QVERIFY(bridge.createMidiClip(0, 0, 4.0));
        QVERIFY(bridge.clipSlotOccupied(0, 0));
    }
};

void TestPianoRoll::initTestCase()
{
    QVERIFY(m_themes.load(QStringLiteral("nylon")));
}

void TestPianoRoll::emptyStateWithoutClip()
{
    ProjectBridge bridge;
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 300);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    QVERIFY(!roll.hasClip());
    QCOMPARE(roll.noteIndexAt(QPoint(100, 100)), -1);
    QVERIFY(!roll.viewport()->grab().isNull());
    QTest::mouseDClick(roll.viewport(), Qt::LeftButton, Qt::NoModifier, QPoint(200, 100));
    QCOMPARE(bridge.trackCount(), 0ull);
}

void TestPianoRoll::geometryRoundTrips()
{
    ProjectBridge bridge;
    prepare(bridge);
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 300);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    roll.setClip(0, 0);
    QVERIFY(roll.hasClip());
    roll.verticalScrollBar()->setValue(0);
    roll.horizontalScrollBar()->setValue(0);
    MidiNote n{};
    n.pitch = 127;
    n.velocity = 100;
    n.startBeats = 1.0;
    n.lengthBeats = 0.5;
    const QRect r = roll.noteRect(n);
    QCOMPARE(r.x(), roll.keyboardWidth() + roll.pixelsPerBeat());
    QCOMPARE(r.y(), roll.rulerHeight() + 1);
    QCOMPARE(r.width(), roll.pixelsPerBeat() / 2 - 1);
    const QPointF cell = roll.cellAt(r.center());
    QCOMPARE(static_cast<int>(cell.y()), 127);
    QVERIFY(cell.x() > 1.0 && cell.x() < 1.5);
}

void TestPianoRoll::doubleClickAddsAndRemovesNotes()
{
    ProjectBridge bridge;
    prepare(bridge);
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 300);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    roll.setClip(0, 0);
    roll.verticalScrollBar()->setValue(0);
    roll.horizontalScrollBar()->setValue(0);
    QSignalSpy selected(&roll, &PianoRoll::noteSelected);
    // Row 2 from the top = pitch 125, at beat 2.
    const QPoint cell(roll.keyboardWidth() + 2 * roll.pixelsPerBeat() + 3, roll.rulerHeight() + 2 * roll.rowHeight() + 3);
    QTest::mouseDClick(roll.viewport(), Qt::LeftButton, Qt::NoModifier, cell);
    QCOMPARE(bridge.clipNoteCount(0, 0), 1ull);
    MidiNote note{};
    QVERIFY(bridge.clipNote(0, 0, 0, note));
    QCOMPARE(static_cast<int>(note.pitch), 125);
    QCOMPARE(note.startBeats, 2.0);
    QCOMPARE(note.lengthBeats, roll.gridBeats());
    QCOMPARE(roll.selectedNote(), 0);
    QCOMPARE(selected.count(), 1);
    QCOMPARE(roll.noteIndexAt(roll.noteRect(note).center()), 0);
    // Undo removes it, redo brings it back.
    QVERIFY(bridge.undo());
    QCOMPARE(bridge.clipNoteCount(0, 0), 0ull);
    QVERIFY(bridge.redo());
    QCOMPARE(bridge.clipNoteCount(0, 0), 1ull);
    // Double-clicking the note removes it.
    QTest::mouseDClick(roll.viewport(), Qt::LeftButton, Qt::NoModifier, roll.noteRect(note).center());
    QCOMPARE(bridge.clipNoteCount(0, 0), 0ull);
    QCOMPARE(roll.selectedNote(), -1);
}

void TestPianoRoll::dragMovesNoteQuantized()
{
    ProjectBridge bridge;
    prepare(bridge);
    MidiNote n{};
    n.pitch = 120;
    n.velocity = 90;
    n.startBeats = 1.0;
    n.lengthBeats = 1.0;
    QVERIFY(bridge.addClipNote(0, 0, n));
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 300);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    roll.setClip(0, 0);
    roll.verticalScrollBar()->setValue(0);
    roll.horizontalScrollBar()->setValue(0);
    const QPoint from = roll.noteRect(n).center();
    // One beat right and two rows down, plus a little slop that quantizes away.
    const QPoint to = from + QPoint(roll.pixelsPerBeat() + 3, 2 * roll.rowHeight() + 1);
    QTest::mousePress(roll.viewport(), Qt::LeftButton, Qt::NoModifier, from);
    QCOMPARE(roll.selectedNote(), 0);
    QTest::mouseMove(roll.viewport(), to);
    QTest::mouseRelease(roll.viewport(), Qt::LeftButton, Qt::NoModifier, to);
    MidiNote moved{};
    QVERIFY(bridge.clipNote(0, 0, 0, moved));
    QCOMPARE(moved.startBeats, 2.0);
    QCOMPARE(static_cast<int>(moved.pitch), 118);
    QCOMPARE(moved.lengthBeats, 1.0);
    QVERIFY(bridge.undo());
    QVERIFY(bridge.clipNote(0, 0, 0, moved));
    QCOMPARE(moved.startBeats, 1.0);
}

void TestPianoRoll::deleteKeyRemovesSelection()
{
    ProjectBridge bridge;
    prepare(bridge);
    MidiNote n{};
    n.pitch = 100;
    n.velocity = 90;
    n.startBeats = 0.0;
    n.lengthBeats = 0.5;
    QVERIFY(bridge.addClipNote(0, 0, n));
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 300);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    roll.setClip(0, 0);
    QTest::keyClick(&roll, Qt::Key_Delete);
    QCOMPARE(bridge.clipNoteCount(0, 0), 1ull);
    roll.selectNote(0);
    QTest::keyClick(&roll, Qt::Key_Delete);
    QCOMPARE(bridge.clipNoteCount(0, 0), 0ull);
    QCOMPARE(roll.selectedNote(), -1);
}

void TestPianoRoll::refreshDropsStaleSelection()
{
    ProjectBridge bridge;
    prepare(bridge);
    MidiNote n{};
    n.pitch = 64;
    n.velocity = 64;
    n.startBeats = 0.0;
    n.lengthBeats = 1.0;
    QVERIFY(bridge.addClipNote(0, 0, n));
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.setClip(0, 0);
    roll.selectNote(0);
    QVERIFY(bridge.removeClipNote(0, 0, 0));
    QCOMPARE(roll.selectedNote(), -1);
    QVERIFY(bridge.deleteClip(0, 0));
    QVERIFY(!roll.hasClip());
}

void TestPianoRoll::resizeGripChangesLength()
{
    ProjectBridge bridge;
    prepare(bridge);
    MidiNote n{};
    n.pitch = 120;
    n.velocity = 90;
    n.startBeats = 1.0;
    n.lengthBeats = 1.0;
    QVERIFY(bridge.addClipNote(0, 0, n));
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 320);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    roll.setClip(0, 0);
    roll.verticalScrollBar()->setValue(0);
    roll.horizontalScrollBar()->setValue(0);
    const QPoint grip = roll.noteResizeGrip(n).center();
    QVERIFY(roll.noteRect(n).contains(grip));
    QTest::mousePress(roll.viewport(), Qt::LeftButton, Qt::NoModifier, grip);
    QTest::mouseMove(roll.viewport(), grip + QPoint(roll.pixelsPerBeat(), 0));
    QTest::mouseRelease(roll.viewport(), Qt::LeftButton, Qt::NoModifier, grip + QPoint(roll.pixelsPerBeat(), 0));
    MidiNote after{};
    QVERIFY(bridge.clipNote(0, 0, 0, after));
    QCOMPARE(after.lengthBeats, 2.0);
    QCOMPARE(after.startBeats, 1.0);
    QCOMPARE(static_cast<int>(after.pitch), 120);
    // Shrinking never goes below one grid step.
    const QPoint grip2 = roll.noteResizeGrip(after).center();
    QTest::mousePress(roll.viewport(), Qt::LeftButton, Qt::NoModifier, grip2);
    QTest::mouseMove(roll.viewport(), grip2 - QPoint(10 * roll.pixelsPerBeat(), 0));
    QTest::mouseRelease(roll.viewport(), Qt::LeftButton, Qt::NoModifier, grip2 - QPoint(10 * roll.pixelsPerBeat(), 0));
    QVERIFY(bridge.clipNote(0, 0, 0, after));
    QCOMPARE(after.lengthBeats, roll.gridBeats());
}

void TestPianoRoll::velocityLaneDragChangesVelocity()
{
    ProjectBridge bridge;
    prepare(bridge);
    MidiNote n{};
    n.pitch = 60;
    n.velocity = 64;
    n.startBeats = 0.5;
    n.lengthBeats = 1.0;
    QVERIFY(bridge.addClipNote(0, 0, n));
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.resize(600, 320);
    roll.show();
    QVERIFY(QTest::qWaitForWindowExposed(&roll));
    roll.setClip(0, 0);
    roll.horizontalScrollBar()->setValue(0);
    const QRect bar = roll.velocityBar(n);
    QVERIFY(bar.y() >= roll.viewport()->height() - roll.velocityLaneHeight());
    const QPoint top(bar.center().x(), bar.y() + 1);
    QTest::mousePress(roll.viewport(), Qt::LeftButton, Qt::NoModifier, top);
    QCOMPARE(roll.selectedNote(), 0);
    QTest::mouseMove(roll.viewport(), top - QPoint(0, 20));
    QTest::mouseRelease(roll.viewport(), Qt::LeftButton, Qt::NoModifier, top - QPoint(0, 20));
    MidiNote after{};
    QVERIFY(bridge.clipNote(0, 0, 0, after));
    QCOMPARE(static_cast<int>(after.velocity), 84);
    QCOMPARE(after.startBeats, 0.5);
    // Double-clicking in the lane creates nothing.
    QTest::mouseDClick(roll.viewport(), Qt::LeftButton, Qt::NoModifier, QPoint(300, roll.viewport()->height() - 10));
    QCOMPARE(bridge.clipNoteCount(0, 0), 1ull);
}

void TestPianoRoll::controlWheelZooms()
{
    ProjectBridge bridge;
    prepare(bridge);
    PianoRoll roll(&bridge, &m_themes.theme());
    roll.setClip(0, 0);
    const int before = roll.pixelsPerBeat();
    QWheelEvent in(QPointF(200, 100), QPointF(200, 100), QPoint(), QPoint(0, 240), Qt::NoButton, Qt::ControlModifier,
        Qt::NoScrollPhase, false);
    QCoreApplication::sendEvent(roll.viewport(), &in);
    QCOMPARE(roll.pixelsPerBeat(), before + 16);
    QWheelEvent out(QPointF(200, 100), QPointF(200, 100), QPoint(), QPoint(0, -120), Qt::NoButton, Qt::ControlModifier,
        Qt::NoScrollPhase, false);
    QCoreApplication::sendEvent(roll.viewport(), &out);
    QCOMPARE(roll.pixelsPerBeat(), before + 8);
    roll.setZoom(1);
    QCOMPARE(roll.pixelsPerBeat(), 8);
    roll.setZoom(9999);
    QCOMPARE(roll.pixelsPerBeat(), 400);
}

QTEST_MAIN(TestPianoRoll)
#include "test_pianoroll.moc"
