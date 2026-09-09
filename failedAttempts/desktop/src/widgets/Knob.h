#pragma once

#include "ControlWidget.h"

namespace nylon {

// Rotary control drawn as an arc from a start angle. Bipolar knobs (pan)
// draw the arc from the center instead of the minimum.
class Knob : public ControlWidget {
    Q_OBJECT
public:
    explicit Knob(const Theme* theme, QWidget* parent = nullptr);

    void setBipolar(bool bipolar);
    bool isBipolar() const { return m_bipolar; }
    // Text shown under the arc; when empty the value is formatted with the
    // unit and one decimal.
    void setDisplayText(const QString& text);
    QString displayText() const;

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override { return sizeHint(); }

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    bool m_bipolar = false;
    QString m_displayText;
};

} // namespace nylon
