#pragma once

#include <QtGlobal>

// Integer helpers for view layout. Metrics are validated to at most 100000
// and counts can reach millions, so products are formed in 64 bits and
// clamped before they become widget coordinates or scroll ranges.
namespace nylon::layout {

// Largest scrollable extent per axis, in pixels. Content past this point is
// not reachable by scrolling; it keeps every coordinate far inside `int`.
constexpr qint64 kMaxExtent = qint64(1) << 24;

inline int clampExtent(qint64 value)
{
    return static_cast<int>(qBound<qint64>(0, value, kMaxExtent));
}

// True when `value` can be used as a widget coordinate.
inline bool fitsCoordinate(qint64 value)
{
    return value >= -kMaxExtent && value <= kMaxExtent;
}

// Number of cells of pitch `pitch` that fit inside the maximum extent after
// `origin` pixels of leading content. Caps `count` so extent arithmetic
// stays bounded.
inline qint64 layoutCount(qint64 count, qint64 pitch, qint64 origin)
{
    if (count <= 0 || pitch <= 0) {
        return 0;
    }
    const qint64 room = kMaxExtent - qBound<qint64>(0, origin, kMaxExtent);
    return qMin(count, room / pitch);
}

// Index range [first, last] of cells that intersect a viewport of length
// `viewLength` when the content is scrolled by `scroll` and the first cell
// starts at `origin`. Returns false when no cell is visible.
inline bool visibleRange(qint64 count, qint64 size, qint64 pitch, qint64 origin,
    qint64 scroll, qint64 viewLength, qint64* first, qint64* last)
{
    if (count <= 0 || pitch <= 0 || size <= 0 || viewLength <= 0) {
        return false;
    }
    // Cell i occupies [origin + i*pitch, origin + i*pitch + size) in content
    // space; the viewport is [scroll, scroll + viewLength).
    const qint64 lowContent = scroll - origin - size + 1;
    const qint64 highContent = scroll + viewLength - origin - 1;
    if (highContent < 0) {
        return false;
    }
    qint64 lo = lowContent <= 0 ? 0 : (lowContent + pitch - 1) / pitch;
    qint64 hi = highContent / pitch;
    if (hi >= count) {
        hi = count - 1;
    }
    if (lo > hi) {
        return false;
    }
    *first = lo;
    *last = hi;
    return true;
}

} // namespace nylon::layout
