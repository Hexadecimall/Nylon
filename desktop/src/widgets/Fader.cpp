#include "Fader.h"

#include "PanelPaint.h"
#include "Theme.h"

#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QtMath>

namespace nylon {

namespace {
// Exponent of the position curve. Values under 1 stretch the top of the
// range so 0 dB sits well above the middle of the travel.
constexpr double kCurve = 0.55;
} // namespace

Fader::Fader(const Theme* theme, QWidget* parent)
    : ControlWidget(theme, parent)
{
    setRange(-70.0, 6.0);
    setDefaultValue(0.0);
    setValue(0.0);
    setUnit(QStringLiteral(" dB"));
    setDragPixels(300);
}

double Fader::positionForDecibels(double db, double minDb, double maxDb)
{
    const double span = maxDb - minDb;
    if (span <= 0.0) {
        return 0.0;
    }
    const double linear = qBound(0.0, (db - minDb) / span, 1.0);
    return qPow(linear, 1.0 / kCurve);
}

double Fader::decibelsForPosition(double position, double minDb, double maxDb)
{
    const double linear = qPow(qBound(0.0, position, 1.0), kCurve);
    return minDb + linear * (maxDb - minDb);
}

void Fader::setShowScale(bool show)
{
    m_showScale = show;
    updateGeometry();
    update();
}

QString Fader::displayText() const
{
    if (value() <= minimum()) {
        return QStringLiteral("-inf");
    }
    return QStringLiteral("%1").arg(value(), 0, 'f', 1);
}

QSize Fader::sizeHint() const
{
    const int w = theme()->metricInt(QStringLiteral("fader.width"), 14) + (m_showScale ? 28 : 0);
    return QSize(w + 8, 120);
}

QSize Fader::minimumSizeHint() const
{
    return QSize(sizeHint().width(), 48);
}

QRect Fader::trackRect() const
{
    const int w = theme()->metricInt(QStringLiteral("fader.width"), 14);
    const int handleH = theme()->metricInt(QStringLiteral("fader.handle.height"), 8);
    const int x = m_showScale ? 4 : (width() - w) / 2;
    return QRect(x, handleH / 2 + 2, w, qMax(1, height() - handleH - 4));
}

QRect Fader::handleRect() const
{
    const QRect track = trackRect();
    const int handleH = theme()->metricInt(QStringLiteral("fader.handle.height"), 8);
    const double pos = positionForDecibels(value(), minimum(), maximum());
    const int y = track.bottom() - static_cast<int>(qRound(pos * track.height()));
    return QRect(track.x() - 2, y - handleH / 2, track.width() + 4, handleH);
}

double Fader::positionAtY(int y) const
{
    const QRect track = trackRect();
    if (track.height() <= 0) {
        return 0.0;
    }
    return qBound(0.0, static_cast<double>(track.bottom() - y) / track.height(), 1.0);
}

void Fader::mousePressEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton && isEnabled() && !handleRect().contains(event->pos())) {
        // Click on the track jumps to that position, then drags from there.
        m_handleDrag = true;
        emit dragStarted();
        setValue(decibelsForPosition(positionAtY(event->pos().y()), minimum(), maximum()));
        event->accept();
        return;
    }
    m_handleDrag = false;
    ControlWidget::mousePressEvent(event);
}

void Fader::mouseMoveEvent(QMouseEvent* event)
{
    if (m_handleDrag) {
        setValue(decibelsForPosition(positionAtY(event->pos().y()), minimum(), maximum()));
        event->accept();
        return;
    }
    ControlWidget::mouseMoveEvent(event);
}

void Fader::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const Theme* t = theme();
    p.setRenderHint(QPainter::Antialiasing, true);
    const QRect track = trackRect();
    const double tr = track.width() / 2.0;
    QPainterPath trackPath;
    trackPath.addRoundedRect(QRectF(track), tr, tr);
    paint::control(p, *t, trackPath, t->color(QStringLiteral("fader.track")), true);

    // Fill from the bottom up to the handle.
    const QRect handle = handleRect();
    const QRect fill(track.x(), handle.center().y(), track.width(), track.bottom() - handle.center().y() + 1);
    p.save();
    p.setClipPath(trackPath);
    p.fillRect(fill, isEnabled() ? t->color(QStringLiteral("fader.fill")) : t->color(QStringLiteral("control.disabled")));
    p.restore();

    // Unity mark.
    const double unity = positionForDecibels(0.0, minimum(), maximum());
    const int unityY = track.bottom() - static_cast<int>(qRound(unity * track.height()));
    p.fillRect(QRect(track.x() - 2, unityY, track.width() + 4, 1), t->color(QStringLiteral("text.secondary")));

    QPainterPath handlePath;
    handlePath.addRoundedRect(QRectF(handle).adjusted(0.5, 0.5, -0.5, -0.5), 3, 3);
    paint::control(p, *t, handlePath, isEnabled() ? t->color(QStringLiteral("fader.handle")) : t->color(QStringLiteral("text.disabled")));
    p.fillRect(QRect(handle.x() + 3, handle.center().y(), handle.width() - 6, 1), t->color(QStringLiteral("fader.track")));

    if (m_showScale) {
        p.setPen(t->color(QStringLiteral("text.secondary")));
        QFont f = font();
        f.setPixelSize(qMax(6, t->metricInt(QStringLiteral("font.size"), 11) - 2));
        p.setFont(f);
        const double marks[] = {6.0, 0.0, -6.0, -12.0, -24.0, -48.0};
        for (double db : marks) {
            if (db < minimum() || db > maximum()) {
                continue;
            }
            const double pos = positionForDecibels(db, minimum(), maximum());
            const int y = track.bottom() - static_cast<int>(qRound(pos * track.height()));
            p.fillRect(QRect(track.right() + 2, y, 3, 1), t->color(QStringLiteral("text.secondary")));
            p.drawText(QRect(track.right() + 6, y - 6, width() - track.right() - 6, 12),
                Qt::AlignLeft | Qt::AlignVCenter, QString::number(db, 'f', 0));
        }
    }
}

} // namespace nylon
