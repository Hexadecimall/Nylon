#pragma once

#include "ControlWidget.h"

namespace nylon {

// Vertical gain fader. The value is in decibels; the handle position uses
// a curve that gives the range around 0 dB more travel, matching mixer
// convention. An optional scale draws tick labels beside the track.
class Fader : public ControlWidget {
    Q_OBJECT
public:
    explicit Fader(const Theme* theme, QWidget* parent = nullptr);

    // Maps a decibel value to a 0..1 handle position and back.
    static double positionForDecibels(double db, double minDb, double maxDb);
    static double decibelsForPosition(double position, double minDb, double maxDb);

    void setShowScale(bool show);
    QString displayText() const;

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override;
    // Rectangle of the handle in widget coordinates.
    QRect handleRect() const;

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;

private:
    QRect trackRect() const;
    double positionAtY(int y) const;

    bool m_showScale = false;
    bool m_handleDrag = false;
};

} // namespace nylon
