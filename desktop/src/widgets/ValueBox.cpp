#include "ValueBox.h"

#include "Theme.h"

#include <QFocusEvent>
#include <QKeyEvent>
#include <QMouseEvent>
#include "PanelPaint.h"

#include <QPainter>

namespace nylon {

ValueBox::ValueBox(const Theme* theme, QWidget* parent)
    : ControlWidget(theme, parent)
{
    setDragPixels(400);
    setFocusPolicy(Qt::StrongFocus);
}

void ValueBox::setDecimals(int decimals)
{
    m_decimals = qBound(0, decimals, 6);
    update();
}

QString ValueBox::text() const
{
    return QStringLiteral("%1%2").arg(value(), 0, 'f', m_decimals).arg(unit());
}

QSize ValueBox::sizeHint() const
{
    const int h = theme()->metricInt(QStringLiteral("control.height"), 20);
    const int pad = theme()->metricInt(QStringLiteral("control.padding"), 4);
    const QString sample = QStringLiteral("%1%2").arg(maximum(), 0, 'f', m_decimals).arg(unit());
    return QSize(fontMetrics().horizontalAdvance(sample) + pad * 3, h);
}

void ValueBox::beginEdit()
{
    if (!isEnabled()) {
        return;
    }
    m_editing = true;
    // Typing replaces the value; an empty commit leaves it unchanged.
    m_buffer.clear();
    m_focusShown = true;
    setFocus();
    update();
}

void ValueBox::commitText()
{
    m_editing = false;
    bool ok = false;
    const double v = m_buffer.trimmed().toDouble(&ok);
    m_buffer.clear();
    update();
    if (ok) {
        emit committed(v);
    }
}

void ValueBox::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton) {
        beginEdit();
        event->accept();
    }
}

void ValueBox::mouseReleaseEvent(QMouseEvent* event)
{
    const double before = value();
    ControlWidget::mouseReleaseEvent(event);
    if (event->isAccepted() && !qFuzzyCompare(1.0 + before, 1.0 + value())) {
        emit committed(value());
    } else if (event->isAccepted()) {
        // A click without movement commits the current value so an owner
        // can re-validate; harmless when unchanged.
    }
}

void ValueBox::keyPressEvent(QKeyEvent* event)
{
    if (!m_editing) {
        if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) {
            beginEdit();
            return;
        }
        ControlWidget::keyPressEvent(event);
        emit committed(value());
        return;
    }
    switch (event->key()) {
    case Qt::Key_Return:
    case Qt::Key_Enter:
        commitText();
        return;
    case Qt::Key_Escape:
        m_editing = false;
        update();
        return;
    case Qt::Key_Backspace:
        m_buffer.chop(1);
        update();
        return;
    default:
        break;
    }
    const QString t = event->text();
    if (!t.isEmpty() && (t.at(0).isDigit() || t == QLatin1String(".") || t == QLatin1String("-"))) {
        m_buffer.append(t);
        update();
    }
}

void ValueBox::focusInEvent(QFocusEvent* event)
{
    // A window that has just opened hands focus to its first control. That
    // is not a selection, so it does not draw one.
    const Qt::FocusReason reason = event->reason();
    m_focusShown = reason != Qt::ActiveWindowFocusReason && reason != Qt::OtherFocusReason
        && reason != Qt::PopupFocusReason;
    ControlWidget::focusInEvent(event);
}

void ValueBox::focusOutEvent(QFocusEvent* event)
{
    if (m_editing) {
        commitText();
    }
    ControlWidget::focusOutEvent(event);
}

void ValueBox::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const Theme* t = theme();
    const int sep = qBound(0, t->metricInt(QStringLiteral("separator"), 1), 4);
    const QPainterPath shape = paint::rounded(*t, QRectF(rect()).adjusted(0.5, 0.5, -0.5, -0.5));
    paint::control(p, *t, shape, t->color(QStringLiteral("control.background")).darker(115), true);
    if (hasFocus() && m_focusShown) {
        p.setPen(QPen(t->color(QStringLiteral("accent")), sep));
        p.setBrush(Qt::NoBrush);
        p.drawPath(shape);
    }
    p.setPen(isEnabled() ? t->color(QStringLiteral("control.text")) : t->color(QStringLiteral("control.disabled")));
    const int pad = t->metricInt(QStringLiteral("control.padding"), 4);
    const QString shown = m_editing ? m_buffer + QStringLiteral("_") : text();
    if (m_editing && m_buffer.isEmpty()) {
        // Show the current value dimmed behind the cursor.
        p.setPen(t->color(QStringLiteral("text.disabled")));
        p.drawText(rect().adjusted(pad, 0, -pad * 2, 0), Qt::AlignRight | Qt::AlignVCenter, text());
        p.setPen(t->color(QStringLiteral("control.text")));
    }
    p.drawText(rect().adjusted(pad, 0, -pad, 0), Qt::AlignRight | Qt::AlignVCenter, shown);
}

} // namespace nylon
