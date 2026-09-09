#include "LayoutMath.h"

#include <QtTest>

using namespace nylon::layout;

class TestLayout : public QObject {
    Q_OBJECT
private slots:
    void clampExtentBounds();
    void fitsCoordinateBounds();
    void layoutCountCapsByExtent();
    void visibleRangeBasic();
    void visibleRangeWithScrollAndOrigin();
    void visibleRangeNothingVisible();
    void visibleRangeHandlesExtremeProducts();
};

void TestLayout::clampExtentBounds()
{
    QCOMPARE(clampExtent(-5), 0);
    QCOMPARE(clampExtent(0), 0);
    QCOMPARE(clampExtent(1234), 1234);
    QCOMPARE(clampExtent(kMaxExtent), static_cast<int>(kMaxExtent));
    QCOMPARE(clampExtent(kMaxExtent + 1), static_cast<int>(kMaxExtent));
    QCOMPARE(clampExtent(qint64(100000) * 100000), static_cast<int>(kMaxExtent));
    QCOMPARE(clampExtent(std::numeric_limits<qint64>::max()), static_cast<int>(kMaxExtent));
}

void TestLayout::fitsCoordinateBounds()
{
    QVERIFY(fitsCoordinate(0));
    QVERIFY(fitsCoordinate(-kMaxExtent));
    QVERIFY(fitsCoordinate(kMaxExtent));
    QVERIFY(!fitsCoordinate(kMaxExtent + 1));
    QVERIFY(!fitsCoordinate(-kMaxExtent - 1));
    QVERIFY(!fitsCoordinate(qint64(1) << 40));
}

void TestLayout::layoutCountCapsByExtent()
{
    QCOMPARE(layoutCount(0, 10, 0), qint64(0));
    QCOMPARE(layoutCount(5, 0, 0), qint64(0));
    QCOMPARE(layoutCount(5, 10, 0), qint64(5));
    QCOMPARE(layoutCount(kMaxExtent, 1, 0), kMaxExtent);
    QCOMPARE(layoutCount(kMaxExtent, 2, 0), kMaxExtent / 2);
    QCOMPARE(layoutCount(qint64(1) << 40, 100001, 0), kMaxExtent / 100001);
    // Leading content reduces the room.
    QCOMPARE(layoutCount(1000, 100, kMaxExtent - 250), qint64(2));
    QCOMPARE(layoutCount(1000, 100, kMaxExtent), qint64(0));
    QCOMPARE(layoutCount(1000, 100, kMaxExtent * 4), qint64(0));
    QCOMPARE(layoutCount(1000, 100, -50), qint64(1000));
    // Every laid-out cell ends within the extent.
    const qint64 n = layoutCount(qint64(1) << 40, 100001, 12345);
    QVERIFY(12345 + n * 100001 <= kMaxExtent);
}

void TestLayout::visibleRangeBasic()
{
    qint64 first = -1, last = -1;
    // 10 cells, size 20, pitch 21, no origin/scroll, viewport 100 px:
    // cells 0..4 start at 0,21,42,63,84 and cell 4 ends at 104 > 100.
    QVERIFY(visibleRange(10, 20, 21, 0, 0, 100, &first, &last));
    QCOMPARE(first, qint64(0));
    QCOMPARE(last, qint64(4));
    // Viewport exactly at a boundary: cell 4 starts at 84 < 84? no.
    QVERIFY(visibleRange(10, 20, 21, 0, 0, 84, &first, &last));
    QCOMPARE(last, qint64(3));
    QVERIFY(visibleRange(10, 20, 21, 0, 0, 85, &first, &last));
    QCOMPARE(last, qint64(4));
    // Count limits the last index.
    QVERIFY(visibleRange(3, 20, 21, 0, 0, 1000, &first, &last));
    QCOMPARE(first, qint64(0));
    QCOMPARE(last, qint64(2));
}

void TestLayout::visibleRangeWithScrollAndOrigin()
{
    qint64 first = -1, last = -1;
    // Origin 50, pitch 21, size 20. Scroll 100, viewport 60 -> content
    // window [100, 160). Cell i spans [50+21i, 70+21i).
    // i=2: [92,112) visible. i=1: [71,91) not. i=5: [155,175) visible.
    // i=6: [176,196) not.
    QVERIFY(visibleRange(100, 20, 21, 50, 100, 60, &first, &last));
    QCOMPARE(first, qint64(2));
    QCOMPARE(last, qint64(5));
    // A cell whose last pixel is the first visible pixel counts.
    // i=1 spans [71, 91): scroll 90 still shows its last pixel, scroll 91
    // does not.
    QVERIFY(visibleRange(100, 20, 21, 50, 90, 60, &first, &last));
    QCOMPARE(first, qint64(1));
    QVERIFY(visibleRange(100, 20, 21, 50, 91, 60, &first, &last));
    QCOMPARE(first, qint64(2));
}

void TestLayout::visibleRangeNothingVisible()
{
    qint64 first = 7, last = 7;
    QVERIFY(!visibleRange(0, 20, 21, 0, 0, 100, &first, &last));
    QVERIFY(!visibleRange(10, 0, 21, 0, 0, 100, &first, &last));
    QVERIFY(!visibleRange(10, 20, 0, 0, 0, 100, &first, &last));
    QVERIFY(!visibleRange(10, 20, 21, 0, 0, 0, &first, &last));
    // Viewport entirely above the origin.
    QVERIFY(!visibleRange(10, 20, 21, 500, 0, 100, &first, &last));
    // Viewport entirely past the last cell (10 cells end at 209).
    QVERIFY(!visibleRange(10, 20, 21, 0, 300, 100, &first, &last));
    QCOMPARE(first, qint64(7));
    QCOMPARE(last, qint64(7));
}

void TestLayout::visibleRangeHandlesExtremeProducts()
{
    qint64 first = -1, last = -1;
    const qint64 count = qint64(1) << 20;
    const qint64 size = 100000;
    // Products reach 10^11; the result must be a small window regardless.
    QVERIFY(visibleRange(count, size, size + 1, 0, 0, 1000, &first, &last));
    QCOMPARE(first, qint64(0));
    QCOMPARE(last, qint64(0));
    QVERIFY(visibleRange(count, size, size + 1, 0, kMaxExtent, 1000, &first, &last));
    QCOMPARE(first, kMaxExtent / (size + 1));
    QVERIFY(last - first <= 1);
}

QTEST_GUILESS_MAIN(TestLayout)
#include "test_layout.moc"
