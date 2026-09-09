#include "Theme.h"
#include "ThemeManager.h"
#include "widgets/Fader.h"
#include "widgets/FlatButton.h"
#include "widgets/Knob.h"
#include "widgets/LevelMeter.h"
#include "widgets/ValueBox.h"

#include <QtTest>

using namespace nylon;

namespace {
// A grab carries the screen's device pixel ratio, so a point in widget
// coordinates is not a pixel in the image.
QColor pixelAt(const QImage& image, int x, int y)
{
    const qreal ratio = image.devicePixelRatio();
    return image.pixelColor(qRound(static_cast<qreal>(x) * ratio), qRound(static_cast<qreal>(y) * ratio));
}
} // namespace

class TestWidgets : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void controlClampsAndSignalsOnce();
    void faderCurveIsMonotonicAndInvertible();
    void faderDisplayText();
    void faderDragChangesValue();
    void knobBipolarAndDisplay();
    void levelMeterLatchesClip();
    void flatButtonPaintsActiveColor();
    void valueBoxTypedEntryCommits();
    void disabledControlsIgnoreInput();

private:
    ThemeManager m_themes;
};

void TestWidgets::initTestCase()
{
    QVERIFY(m_themes.load(QStringLiteral("nylon")));
}

void TestWidgets::controlClampsAndSignalsOnce()
{
    Knob k(&m_themes.theme());
    k.setRange(0.0, 10.0);
    QSignalSpy spy(&k, &Knob::valueChanged);
    k.setValue(5.0);
    QCOMPARE(spy.count(), 1);
    k.setValue(5.0);
    QCOMPARE(spy.count(), 1);
    k.setValue(50.0);
    QCOMPARE(k.value(), 10.0);
    k.setValue(-3.0);
    QCOMPARE(k.value(), 0.0);
    k.setNormalized(0.25);
    QCOMPARE(k.value(), 2.5);
    QCOMPARE(k.normalized(), 0.25);
    k.setDefaultValue(7.0);
    k.resetToDefault();
    QCOMPARE(k.value(), 7.0);
}

void TestWidgets::faderCurveIsMonotonicAndInvertible()
{
    double last = -1.0;
    for (int db = -70; db <= 6; ++db) {
        const double pos = Fader::positionForDecibels(db, -70.0, 6.0);
        QVERIFY(pos > last);
        last = pos;
        QVERIFY(qAbs(Fader::decibelsForPosition(pos, -70.0, 6.0) - db) < 1e-9);
    }
    QCOMPARE(Fader::positionForDecibels(-70.0, -70.0, 6.0), 0.0);
    QCOMPARE(Fader::positionForDecibels(6.0, -70.0, 6.0), 1.0);
    // Unity sits in the upper part of the travel.
    QVERIFY(Fader::positionForDecibels(0.0, -70.0, 6.0) > 0.75);
}

void TestWidgets::faderDisplayText()
{
    Fader f(&m_themes.theme());
    QCOMPARE(f.value(), 0.0);
    QCOMPARE(f.displayText(), QStringLiteral("0.0"));
    f.setValue(-70.0);
    QCOMPARE(f.displayText(), QStringLiteral("-inf"));
    f.setValue(-6.02);
    QCOMPARE(f.displayText(), QStringLiteral("-6.0"));
}

void TestWidgets::faderDragChangesValue()
{
    Fader f(&m_themes.theme());
    f.resize(40, 200);
    f.show();
    QVERIFY(QTest::qWaitForWindowExposed(&f));
    QSignalSpy started(&f, &Fader::dragStarted);
    QSignalSpy finished(&f, &Fader::dragFinished);
    const QRect handle = f.handleRect();
    QTest::mousePress(&f, Qt::LeftButton, Qt::NoModifier, handle.center());
    QTest::mouseMove(&f, handle.center() + QPoint(0, 60));
    QTest::mouseRelease(&f, Qt::LeftButton, Qt::NoModifier, handle.center() + QPoint(0, 60));
    QVERIFY(f.value() < 0.0);
    QCOMPARE(started.count(), 1);
    QCOMPARE(finished.count(), 1);
    // Clicking the track jumps there.
    const QRect after = f.handleRect();
    QTest::mouseClick(&f, Qt::LeftButton, Qt::NoModifier, QPoint(after.center().x(), 190));
    QVERIFY(f.value() < -40.0);
    QTest::mouseDClick(&f, Qt::LeftButton, Qt::NoModifier, f.handleRect().center());
    QCOMPARE(f.value(), 0.0);
}

void TestWidgets::knobBipolarAndDisplay()
{
    Knob k(&m_themes.theme());
    k.setRange(-1.0, 1.0);
    k.setBipolar(true);
    k.setValue(0.0);
    QVERIFY(k.isBipolar());
    QCOMPARE(k.normalized(), 0.5);
    k.setUnit(QStringLiteral("%"));
    k.setValue(0.5);
    QCOMPARE(k.displayText(), QStringLiteral("0.5%"));
    k.setDisplayText(QStringLiteral("25R"));
    QCOMPARE(k.displayText(), QStringLiteral("25R"));
    k.resize(k.sizeHint());
    QVERIFY(!k.grab().isNull());
}

void TestWidgets::levelMeterLatchesClip()
{
    LevelMeter m(&m_themes.theme());
    QCOMPARE(m.channelCount(), 2);
    QVERIFY(m.isSilent());
    m.setLevels(0, -12.0, -18.0);
    QVERIFY(!m.isSilent());
    QCOMPARE(m.peakDb(0), -12.0);
    QCOMPARE(m.rmsDb(0), -18.0);
    QVERIFY(!m.isClipping(0));
    m.setLevels(1, 1.5, -3.0);
    QVERIFY(m.isClipping(1));
    m.setLevels(1, -20.0, -30.0);
    QVERIFY(m.isClipping(1));
    m.clearClip();
    QVERIFY(!m.isClipping(1));
    // RMS never exceeds peak and nothing goes below the floor.
    m.setLevels(0, -30.0, -10.0);
    QCOMPARE(m.rmsDb(0), -30.0);
    m.setLevels(0, -200.0, -200.0);
    QCOMPARE(m.peakDb(0), m.floorDb());
    m.setLevels(5, 0.0, 0.0);
    QCOMPARE(m.channelCount(), 2);
    m.resize(m.sizeHint());
    QVERIFY(!m.grab().isNull());
}

void TestWidgets::flatButtonPaintsActiveColor()
{
    const Theme& t = m_themes.theme();
    FlatButton b(&t);
    b.setText(QStringLiteral("S"));
    b.setCheckable(true);
    b.setActiveColorKey(QStringLiteral("state.solo"));
    b.resize(40, 20);
    // Antialiasing at the outline can move edge pixels slightly.
    auto near = [](const QColor& a, const QColor& b) {
        return qAbs(a.red() - b.red()) <= 40 && qAbs(a.green() - b.green()) <= 40 && qAbs(a.blue() - b.blue()) <= 40;
    };
    QImage off = b.grab().toImage();
    QVERIFY2(near(pixelAt(off, 6, 10), t.color(QStringLiteral("control.background"))),
        qPrintable(pixelAt(off, 6, 10).name()));
    b.setChecked(true);
    QImage on = b.grab().toImage();
    QVERIFY2(near(pixelAt(on, 6, 10), t.color(QStringLiteral("state.solo"))), qPrintable(pixelAt(on, 6, 10).name()));
    QVERIFY(!near(pixelAt(on, 6, 10), t.color(QStringLiteral("control.background"))));
    b.setChecked(false);
    b.setProminent(true);
    QVERIFY(b.isProminent());
    QImage prominent = b.grab().toImage();
    QVERIFY2(near(pixelAt(prominent, 6, 10), t.color(QStringLiteral("accent"))),
        qPrintable(pixelAt(prominent, 6, 10).name()));
    b.setGlyph(FlatButton::Glyph::Play);
    b.setText(QString());
    b.setSquare(24);
    QCOMPARE(b.sizeHint(), QSize(24, 24));
    QVERIFY(!b.grab().isNull());
}

void TestWidgets::valueBoxTypedEntryCommits()
{
    ValueBox v(&m_themes.theme());
    v.setRange(20.0, 999.0);
    v.setDecimals(2);
    v.setUnit(QStringLiteral(" BPM"));
    v.setValue(120.0);
    QCOMPARE(v.text(), QStringLiteral("120.00 BPM"));
    v.show();
    QVERIFY(QTest::qWaitForWindowExposed(&v));
    QSignalSpy committed(&v, &ValueBox::committed);
    v.beginEdit();
    QVERIFY(v.isEditing());
    QTest::keyClicks(&v, QStringLiteral("140.5"));
    QTest::keyClick(&v, Qt::Key_Return);
    QVERIFY(!v.isEditing());
    QCOMPARE(committed.count(), 1);
    QCOMPARE(committed.first().first().toDouble(), 140.5);
    // The box itself does not change until the owner accepts the value.
    QCOMPARE(v.value(), 120.0);
    v.beginEdit();
    QTest::keyClicks(&v, QStringLiteral("abc"));
    QTest::keyClick(&v, Qt::Key_Escape);
    QCOMPARE(committed.count(), 1);
    QCOMPARE(v.value(), 120.0);
    // An empty commit changes nothing.
    v.beginEdit();
    QTest::keyClick(&v, Qt::Key_Return);
    QCOMPARE(committed.count(), 1);
    QVERIFY(!v.isEditing());
}

void TestWidgets::disabledControlsIgnoreInput()
{
    Knob k(&m_themes.theme());
    k.setRange(0.0, 1.0);
    k.setValue(0.5);
    k.setEnabled(false);
    k.resize(k.sizeHint());
    k.show();
    QVERIFY(QTest::qWaitForWindowExposed(&k));
    QSignalSpy spy(&k, &Knob::valueChanged);
    QTest::mousePress(&k, Qt::LeftButton, Qt::NoModifier, k.rect().center());
    QTest::mouseMove(&k, k.rect().center() + QPoint(0, -50));
    QTest::mouseRelease(&k, Qt::LeftButton, Qt::NoModifier, k.rect().center() + QPoint(0, -50));
    QCOMPARE(spy.count(), 0);
    QCOMPARE(k.value(), 0.5);
}

QTEST_MAIN(TestWidgets)
#include "test_widgets.moc"
