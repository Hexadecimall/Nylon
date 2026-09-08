#pragma once

#include <QWidget>

namespace nylon {

class Theme;

// Vertical stereo peak/RMS meter with a clip indicator. Levels are given in
// decibels; anything at or below the floor draws nothing. There is no
// ballistics here: the owner feeds already-smoothed values.
class LevelMeter : public QWidget {
    Q_OBJECT
public:
    explicit LevelMeter(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setChannelCount(int channels);
    int channelCount() const { return static_cast<int>(m_peak.size()); }
    void setFloor(double db);
    double floorDb() const { return m_floor; }

    // Sets one channel's levels. Values above 0 dB latch the clip indicator
    // until clearClip.
    void setLevels(int channel, double peakDb, double rmsDb);
    double peakDb(int channel) const { return m_peak.value(channel, m_floor); }
    double rmsDb(int channel) const { return m_rms.value(channel, m_floor); }
    bool isClipping(int channel) const { return m_clip.value(channel, false); }
    void clearClip();
    // True when every channel sits at the floor.
    bool isSilent() const;

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override;

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;

private:
    double fraction(double db) const;

    const Theme* m_theme;
    double m_floor = -60.0;
    QList<double> m_peak;
    QList<double> m_rms;
    QList<bool> m_clip;
};

} // namespace nylon
