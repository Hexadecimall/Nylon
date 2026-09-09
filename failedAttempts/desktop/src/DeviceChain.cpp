#include "DeviceChain.h"

#include "PanelPaint.h"
#include "Theme.h"
#include "widgets/Icons.h"

#include <QPainter>
#include <QPaintEvent>

namespace nylon {

namespace {
// Slot geometry. A chain reads left to right, the way signal flows.
constexpr int kMargin = 10;
constexpr int kGap = 8;
constexpr int kSlotWidth = 168;
constexpr int kTitleHeight = 22;
} // namespace

DeviceChain::DeviceChain(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
{
    setObjectName(QStringLiteral("deviceChain"));
    setAutoFillBackground(false);
}

void DeviceChain::setTheme(const Theme* theme)
{
    m_theme = theme;
    update();
}

void DeviceChain::setTrack(const QString& name, const QColor& color, bool instrumentSlot)
{
    m_name = name;
    m_color = color;
    m_instrument = instrumentSlot;
    update();
}

int DeviceChain::slotCount() const
{
    if (m_name.isEmpty()) {
        return 0;
    }
    // An instrument track carries a sound source before its effects.
    return m_instrument ? 3 : 2;
}

QRect DeviceChain::slotRect(int index) const
{
    if (index < 0 || index >= slotCount()) {
        return QRect();
    }
    const int top = kMargin;
    const int bottom = qMax(top + kTitleHeight * 2, height() - kMargin);
    return QRect(kMargin + index * (kSlotWidth + kGap), top, kSlotWidth, bottom - top);
}

QSize DeviceChain::sizeHint() const
{
    const int visible = qMax(1, slotCount());
    return QSize(kMargin * 2 + visible * kSlotWidth + (visible - 1) * kGap, 140);
}

void DeviceChain::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing, true);
    const QColor secondary = m_theme->color(QStringLiteral("text.secondary"));
    const QColor disabled = m_theme->color(QStringLiteral("text.disabled"));

    if (m_name.isEmpty()) {
        p.setPen(secondary);
        p.drawText(rect(), Qt::AlignCenter, tr("Select a track to see its devices."));
        return;
    }

    const int count = slotCount();
    for (int index = 0; index < count; ++index) {
        const QRect slot = slotRect(index);
        if (slot.isNull() || slot.right() > width()) {
            break;
        }
        const bool instrument = m_instrument && index == 0;
        const QString label = instrument ? tr("Instrument") : tr("Audio effect");
        const Icon glyph = instrument ? Icon::Instrument : Icon::Effect;

        // The slot itself: a dashed outline, because nothing is loaded and
        // a solid one would read as a device that failed to draw.
        const QPainterPath shape = paint::rounded(*m_theme, QRectF(slot), false);
        p.fillPath(shape, m_theme->color(QStringLiteral("panel")));
        QPen outline(disabled, 1, Qt::DashLine);
        p.setPen(outline);
        p.setBrush(Qt::NoBrush);
        p.drawPath(shape);

        // Title strip in the track's colour, so a chain belongs to a track
        // at a glance.
        QColor tint = m_color.isValid() ? m_color : secondary;
        tint.setAlpha(index == 0 ? 190 : 120);
        p.save();
        p.setClipPath(shape);
        p.fillRect(QRect(slot.left(), slot.top(), slot.width(), kTitleHeight), tint);
        p.restore();
        p.setPen(m_theme->color(QStringLiteral("track.text")));
        p.drawText(QRect(slot.left() + 8, slot.top(), slot.width() - 16, kTitleHeight),
            Qt::AlignLeft | Qt::AlignVCenter, label);

        const QRect body(slot.left(), slot.top() + kTitleHeight, slot.width(), slot.height() - kTitleHeight);
        const QRect mark(body.center().x() - 11, body.center().y() - 16, 22, 22);
        paintIcon(p, glyph, QRectF(mark), disabled);
        p.setPen(disabled);
        p.drawText(QRect(body.left() + 6, mark.bottom() + 4, body.width() - 12, 18),
            Qt::AlignHCenter | Qt::AlignTop, tr("Empty"));
    }

    const QRect hint(kMargin, height() - kMargin - 16, width() - kMargin * 2, 16);
    if (hint.top() > slotRect(0).bottom()) {
        p.setPen(secondary);
        p.drawText(hint, Qt::AlignLeft | Qt::AlignVCenter,
            tr("Drag a device from the browser onto a slot."));
    }
}

} // namespace nylon
