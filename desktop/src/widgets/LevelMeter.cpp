#include "LevelMeter.h"

#include "Theme.h"

#include <QPainter>

namespace nylon {

LevelMeter::LevelMeter(const Theme* theme, QWidget* parent)
    : QWidget(parent)
    , m_theme(theme)
{
    setChannelCount(2);
    setToolTip(tr("Output level. Click to reset the clip indicator."));
}

void LevelMeter::setTheme(const Theme* theme)
{
    m_theme = theme;
    update();
}

void LevelMeter::setChannelCount(int channels)
{
    channels = qBound(1, channels, 16);
    m_peak = QList<double>(channels, m_floor);
    m_rms = QList<double>(channels, m_floor);
    m_clip = QList<bool>(channels, false);
    updateGeometry();
    update();
}

void LevelMeter::setFloor(double db)
{
    m_floor = qMin(db, -1.0);
    for (int i = 0; i < m_peak.size(); ++i) {
        m_peak[i] = qMax(m_peak[i], m_floor);
        m_rms[i] = qMax(m_rms[i], m_floor);
    }
    update();
}

void LevelMeter::setLevels(int channel, double peakDb, double rmsDb)
{
    if (channel < 0 || channel >= m_peak.size()) {
        return;
    }
    m_peak[channel] = qMax(m_floor, peakDb);
    m_rms[channel] = qMax(m_floor, qMin(rmsDb, peakDb));
    if (peakDb > 0.0) {
        m_clip[channel] = true;
    }
    update();
}

void LevelMeter::clearClip()
{
    for (int i = 0; i < m_clip.size(); ++i) {
        m_clip[i] = false;
    }
    update();
}

bool LevelMeter::isSilent() const
{
    for (double v : m_peak) {
        if (v > m_floor) {
            return false;
        }
    }
    return true;
}

QSize LevelMeter::sizeHint() const
{
    const int w = m_theme->metricInt(QStringLiteral("meter.channel.width"), 4);
    return QSize(channelCount() * (w + 1) + 1, 120);
}

QSize LevelMeter::minimumSizeHint() const
{
    return QSize(sizeHint().width(), 24);
}

double LevelMeter::fraction(double db) const
{
    if (db <= m_floor) {
        return 0.0;
    }
    return qBound(0.0, (db - m_floor) / (0.0 - m_floor), 1.0);
}

void LevelMeter::mousePressEvent(QMouseEvent*)
{
    clearClip();
}

void LevelMeter::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const int clipH = m_theme->metricInt(QStringLiteral("meter.clip.height"), 4);
    const int w = m_theme->metricInt(QStringLiteral("meter.channel.width"), 4);
    const QColor background = m_theme->color(QStringLiteral("meter.background"));
    const QColor rms = m_theme->color(QStringLiteral("meter.rms"));
    const QColor peak = m_theme->color(QStringLiteral("meter.peak"));
    const QColor clip = m_theme->color(QStringLiteral("meter.clip"));
    const int top = clipH + 2;
    const int barH = qMax(1, height() - top);
    for (int ch = 0; ch < channelCount(); ++ch) {
        const int x = 1 + ch * (w + 1);
        p.fillRect(QRect(x, 0, w, clipH), m_clip.at(ch) ? clip : background);
        p.fillRect(QRect(x, top, w, barH), background);
        const int rmsH = static_cast<int>(fraction(m_rms.at(ch)) * barH);
        if (rmsH > 0) {
            p.fillRect(QRect(x, top + barH - rmsH, w, rmsH), rms);
        }
        const int peakY = top + barH - static_cast<int>(fraction(m_peak.at(ch)) * barH);
        if (m_peak.at(ch) > m_floor) {
            p.fillRect(QRect(x, qMax(top, peakY - 1), w, 1), peak);
        }
    }
}

} // namespace nylon
