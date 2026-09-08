#pragma once

#include <QWidget>

namespace nylon {

class Theme;

// Base for custom-painted controls. Holds the theme pointer every control
// paints from and a normalized value in [0, 1] mapped onto [min, max].
class ControlWidget : public QWidget {
    Q_OBJECT
public:
    explicit ControlWidget(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    const Theme* theme() const { return m_theme; }

    double minimum() const { return m_min; }
    double maximum() const { return m_max; }
    double value() const { return m_value; }
    double defaultValue() const { return m_default; }
    // Value as a fraction of the range.
    double normalized() const;
    QString label() const { return m_label; }
    QString unit() const { return m_unit; }

    void setRange(double minimum, double maximum);
    void setDefaultValue(double value);
    void setLabel(const QString& label);
    void setUnit(const QString& unit);
    // Drag sensitivity: full range over this many pixels.
    void setDragPixels(int pixels) { m_dragPixels = qMax(1, pixels); }

public slots:
    // Clamps to the range. Emits valueChanged only when the value differs.
    void setValue(double value);
    void setNormalized(double fraction);
    void resetToDefault();

signals:
    void valueChanged(double value);
    // Emitted when the user starts and stops dragging so an owner can group
    // the edits into one undo step.
    void dragStarted();
    void dragFinished();

protected:
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

    // Axis the drag reads: vertical drags move up to increase.
    virtual Qt::Orientation dragOrientation() const { return Qt::Vertical; }

private:
    const Theme* m_theme;
    double m_min = 0.0;
    double m_max = 1.0;
    double m_value = 0.0;
    double m_default = 0.0;
    int m_dragPixels = 200;
    QString m_label;
    QString m_unit;
    bool m_dragging = false;
    QPoint m_dragOrigin;
    double m_dragStartValue = 0.0;
};

} // namespace nylon
