#include "ControlWidget.h"

#include "Theme.h"

#include <QKeyEvent>
#include <QMouseEvent>
#include <QWheelEvent>

namespace nylon {

ControlWidget::ControlWidget(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
{
    setFocusPolicy(Qt::ClickFocus);
    setCursor(Qt::ArrowCursor);
}

void ControlWidget::setTheme(const Theme* theme)
{
    m_theme = theme;
    update();
}

double ControlWidget::normalized() const
{
    const double span = m_max - m_min;
    return span > 0.0 ? (m_value - m_min) / span : 0.0;
}

void ControlWidget::setRange(double minimum, double maximum)
{
    if (maximum < minimum) {
        qSwap(minimum, maximum);
    }
    m_min = minimum;
    m_max = maximum;
    setValue(m_value);
    m_default = qBound(m_min, m_default, m_max);
    update();
}

void ControlWidget::setDefaultValue(double value)
{
    m_default = qBound(m_min, value, m_max);
}

void ControlWidget::setLabel(const QString& label)
{
    m_label = label;
    update();
}

void ControlWidget::setUnit(const QString& unit)
{
    m_unit = unit;
    update();
}

void ControlWidget::setValue(double value)
{
    const double clamped = qBound(m_min, value, m_max);
    if (qFuzzyCompare(1.0 + clamped, 1.0 + m_value)) {
        return;
    }
    m_value = clamped;
    update();
    emit valueChanged(m_value);
}

void ControlWidget::setNormalized(double fraction)
{
    setValue(m_min + qBound(0.0, fraction, 1.0) * (m_max - m_min));
}

void ControlWidget::resetToDefault()
{
    setValue(m_default);
}

void ControlWidget::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton || !isEnabled()) {
        event->ignore();
        return;
    }
    m_dragging = true;
    m_dragOrigin = event->pos();
    m_dragStartValue = m_value;
    setCursor(Qt::BlankCursor);
    emit dragStarted();
    event->accept();
}

void ControlWidget::mouseMoveEvent(QMouseEvent* event)
{
    if (!m_dragging) {
        event->ignore();
        return;
    }
    const QPoint delta = event->pos() - m_dragOrigin;
    const int pixels = dragOrientation() == Qt::Vertical ? -delta.y() : delta.x();
    double step = static_cast<double>(pixels) / m_dragPixels;
    if (event->modifiers() & Qt::ShiftModifier) {
        step *= 0.1;
    }
    setValue(m_dragStartValue + step * (m_max - m_min));
    event->accept();
}

void ControlWidget::mouseReleaseEvent(QMouseEvent* event)
{
    if (!m_dragging) {
        event->ignore();
        return;
    }
    m_dragging = false;
    setCursor(Qt::ArrowCursor);
    emit dragFinished();
    event->accept();
}

void ControlWidget::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton && isEnabled()) {
        emit dragStarted();
        resetToDefault();
        emit dragFinished();
        event->accept();
    }
}

void ControlWidget::wheelEvent(QWheelEvent* event)
{
    if (!isEnabled()) {
        event->ignore();
        return;
    }
    const int steps = event->angleDelta().y() / 120;
    if (steps == 0) {
        event->ignore();
        return;
    }
    double fraction = 0.02 * steps;
    if (event->modifiers() & Qt::ShiftModifier) {
        fraction *= 0.1;
    }
    emit dragStarted();
    setNormalized(normalized() + fraction);
    emit dragFinished();
    event->accept();
}

void ControlWidget::keyPressEvent(QKeyEvent* event)
{
    double fraction = 0.0;
    switch (event->key()) {
    case Qt::Key_Up:
    case Qt::Key_Right:
        fraction = 0.01;
        break;
    case Qt::Key_Down:
    case Qt::Key_Left:
        fraction = -0.01;
        break;
    case Qt::Key_PageUp:
        fraction = 0.1;
        break;
    case Qt::Key_PageDown:
        fraction = -0.1;
        break;
    case Qt::Key_Home:
        setValue(m_min);
        return;
    case Qt::Key_End:
        setValue(m_max);
        return;
    default:
        QWidget::keyPressEvent(event);
        return;
    }
    emit dragStarted();
    setNormalized(normalized() + fraction);
    emit dragFinished();
}

} // namespace nylon
