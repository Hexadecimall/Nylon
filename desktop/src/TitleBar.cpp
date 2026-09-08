#include "TitleBar.h"

#include "Theme.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QMenuBar>
#include <QMouseEvent>
#include <QPainter>
#include <QWindow>

namespace nylon {

TitleBar::TitleBar(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
    , m_menuBar(new QMenuBar(this))
    , m_title(new QLabel(this))
{
    setObjectName(QStringLiteral("titleBar"));
    setMouseTracking(true);
    m_menuBar->setNativeMenuBar(false);
    m_title->setObjectName(QStringLiteral("windowTitle"));
    m_title->setAlignment(Qt::AlignCenter);
    m_title->setAttribute(Qt::WA_TransparentForMouseEvents);

    auto* layout = new QHBoxLayout(this);
    layout->setSpacing(0);
    layout->addSpacing(0);
    layout->addWidget(m_menuBar, 0, Qt::AlignVCenter);
    layout->addWidget(m_title, 1);
    setTheme(theme);
}

void TitleBar::setTheme(const Theme* theme)
{
    m_theme = theme;
    const int h = theme->metricInt(QStringLiteral("titlebar.height"), 36);
    setFixedHeight(h);
    const int size = theme->metricInt(QStringLiteral("window.control.size"), 12);
    const int pad = theme->metricInt(QStringLiteral("panel.padding"), 8);
    // Space for three controls and their gaps ahead of the menu bar.
    layout()->setContentsMargins(pad + 3 * (size + pad / 2) + pad, 0, pad, 0);
    update();
}

void TitleBar::setTitle(const QString& title)
{
    m_title->setText(title);
}

QString TitleBar::title() const
{
    return m_title->text();
}

QSize TitleBar::sizeHint() const
{
    return QSize(400, height());
}

QRect TitleBar::controlRect(int index) const
{
    const int size = m_theme->metricInt(QStringLiteral("window.control.size"), 12);
    const int pad = m_theme->metricInt(QStringLiteral("panel.padding"), 8);
    const int x = pad + index * (size + pad / 2);
    return QRect(x, (height() - size) / 2, size, size);
}

QRect TitleBar::closeRect() const { return controlRect(0); }
QRect TitleBar::minimizeRect() const { return controlRect(1); }
QRect TitleBar::zoomRect() const { return controlRect(2); }

int TitleBar::controlAt(const QPoint& pos) const
{
    for (int i = 0; i < 3; ++i) {
        if (controlRect(i).adjusted(-2, -2, 2, 2).contains(pos)) {
            return i;
        }
    }
    return -1;
}

void TitleBar::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing, true);
    p.setPen(Qt::NoPen);
    const char* const keys[] = {"window.control.close", "window.control.minimize", "window.control.zoom"};
    const QColor glyph = m_theme->color(QStringLiteral("titlebar.background"));
    for (int i = 0; i < 3; ++i) {
        QColor c = m_theme->color(QLatin1String(keys[i]));
        if (!isActiveWindow()) {
            c = m_theme->color(QStringLiteral("text.disabled"));
        }
        if (m_pressedControl == i) {
            c = c.darker(130);
        }
        p.setBrush(c);
        const QRect r = controlRect(i);
        p.drawEllipse(r);
        if (m_hoverControl >= 0) {
            p.setPen(QPen(glyph, 1.4));
            const QPointF c0 = r.center() + QPointF(0.5, 0.5);
            const double s = r.width() / 4.0;
            if (i == 0) {
                p.drawLine(c0 + QPointF(-s, -s), c0 + QPointF(s, s));
                p.drawLine(c0 + QPointF(-s, s), c0 + QPointF(s, -s));
            } else if (i == 1) {
                p.drawLine(c0 + QPointF(-s, 0), c0 + QPointF(s, 0));
            } else {
                p.drawLine(c0 + QPointF(-s, s), c0 + QPointF(s, -s));
                p.drawLine(c0 + QPointF(-s, s), c0 + QPointF(-s, -s * 0.2));
                p.drawLine(c0 + QPointF(s, -s), c0 + QPointF(s * 0.2, -s));
            }
            p.setPen(Qt::NoPen);
        }
    }
}

void TitleBar::mousePressEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton) {
        event->ignore();
        return;
    }
    m_pressedControl = controlAt(event->pos());
    if (m_pressedControl >= 0) {
        update();
        event->accept();
        return;
    }
    if (QWindow* w = window()->windowHandle()) {
        w->startSystemMove();
    }
    event->accept();
}

void TitleBar::mouseReleaseEvent(QMouseEvent* event)
{
    const int pressed = m_pressedControl;
    m_pressedControl = -1;
    update();
    if (pressed >= 0 && controlAt(event->pos()) == pressed) {
        switch (pressed) {
        case 0:
            emit closeRequested();
            break;
        case 1:
            emit minimizeRequested();
            break;
        default:
            emit zoomRequested();
            break;
        }
    }
    event->accept();
}

void TitleBar::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton && controlAt(event->pos()) < 0) {
        emit zoomRequested();
        event->accept();
    }
}

void TitleBar::mouseMoveEvent(QMouseEvent* event)
{
    const int hover = controlAt(event->pos());
    if (hover != m_hoverControl) {
        m_hoverControl = hover;
        update();
    }
    QWidget::mouseMoveEvent(event);
}

void TitleBar::leaveEvent(QEvent* event)
{
    m_hoverControl = -1;
    update();
    QWidget::leaveEvent(event);
}

} // namespace nylon
