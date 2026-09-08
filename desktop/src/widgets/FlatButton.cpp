#include "FlatButton.h"

#include "Theme.h"

#include <QPainter>
#include <QPainterPath>

namespace nylon {

FlatButton::FlatButton(const Theme* theme, QWidget* parent)
    : QAbstractButton(parent)
    , m_theme(theme)
{
    setFocusPolicy(Qt::NoFocus);
}

void FlatButton::setTheme(const Theme* theme)
{
    m_theme = theme;
    update();
}

void FlatButton::setActiveColorKey(const QString& key)
{
    m_activeKey = key;
    update();
}

void FlatButton::setGlyph(Glyph glyph)
{
    m_glyph = glyph;
    update();
}

void FlatButton::setSquare(int side)
{
    m_square = side;
    updateGeometry();
}

QSize FlatButton::sizeHint() const
{
    const int h = m_theme->metricInt(QStringLiteral("control.height"), 20);
    if (m_square > 0) {
        return QSize(m_square, m_square);
    }
    if (m_glyph != Glyph::None && text().isEmpty()) {
        return QSize(h + 6, h);
    }
    const int pad = m_theme->metricInt(QStringLiteral("control.padding"), 4);
    return QSize(fontMetrics().horizontalAdvance(text()) + pad * 4, h);
}

void FlatButton::enterEvent(QEnterEvent*)
{
    m_hover = true;
    update();
}

void FlatButton::leaveEvent(QEvent*)
{
    m_hover = false;
    update();
}

void FlatButton::paintGlyph(QPainter& p, const QRect& r, const QColor& color) const
{
    p.setRenderHint(QPainter::Antialiasing, true);
    p.setPen(Qt::NoPen);
    p.setBrush(color);
    const int s = qMin(r.width(), r.height()) / 2;
    const QPoint c = r.center();
    switch (m_glyph) {
    case Glyph::Play:
    case Glyph::Triangle: {
        QPolygon tri;
        tri << QPoint(c.x() - s / 2, c.y() - s / 2) << QPoint(c.x() + s / 2 + 1, c.y())
            << QPoint(c.x() - s / 2, c.y() + s / 2 + 1);
        p.drawPolygon(tri);
        break;
    }
    case Glyph::Stop:
    case Glyph::Square:
        p.drawRect(QRect(c.x() - s / 2, c.y() - s / 2, s, s));
        break;
    case Glyph::Record:
    case Glyph::Circle:
        p.drawEllipse(QRect(c.x() - s / 2, c.y() - s / 2, s, s));
        break;
    case Glyph::Loop: {
        p.setBrush(Qt::NoBrush);
        p.setPen(QPen(color, 1.5));
        p.drawRoundedRect(QRectF(c.x() - s / 2.0, c.y() - s / 3.0, s, s / 1.5), 2, 2);
        break;
    }
    case Glyph::Metronome: {
        QPolygon body;
        body << QPoint(c.x() - s / 2, c.y() + s / 2) << QPoint(c.x() + s / 2, c.y() + s / 2)
             << QPoint(c.x() + s / 4, c.y() - s / 2) << QPoint(c.x() - s / 4, c.y() - s / 2);
        p.drawPolygon(body);
        p.setPen(QPen(m_theme->color(QStringLiteral("background")), 1));
        p.drawLine(c.x(), c.y() + s / 3, c.x() + s / 3, c.y() - s / 3);
        break;
    }
    case Glyph::None:
        break;
    }
}

void FlatButton::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const QRect r = rect();
    const int sep = qBound(0, m_theme->metricInt(QStringLiteral("separator"), 1), 4);
    QColor fill = m_theme->color(QStringLiteral("control.background"));
    QColor text = m_theme->color(QStringLiteral("control.text"));
    if (!isEnabled()) {
        text = m_theme->color(QStringLiteral("control.disabled"));
    } else if (isChecked()) {
        fill = m_theme->color(m_activeKey);
        text = m_theme->color(QStringLiteral("accent.text"));
    } else if (isDown()) {
        fill = m_theme->color(QStringLiteral("control.pressed"));
    } else if (m_hover) {
        fill = m_theme->color(QStringLiteral("control.hover"));
    }
    p.fillRect(r, fill);
    p.setPen(QPen(m_theme->color(QStringLiteral("control.border")), sep));
    p.drawRect(r.adjusted(0, 0, -1, -1));

    if (m_glyph != Glyph::None) {
        const int inset = qMax(3, r.height() / 4);
        paintGlyph(p, r.adjusted(inset, inset, -inset, -inset), text);
    }
    if (!this->text().isEmpty()) {
        p.setPen(text);
        p.drawText(r, Qt::AlignCenter, this->text());
    }
}

} // namespace nylon
