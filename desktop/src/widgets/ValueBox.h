#pragma once

#include "ControlWidget.h"

namespace nylon {

// Numeric field that edits by vertical drag or by typing. Shows the value
// with a fixed number of decimals and a suffix.
class ValueBox : public ControlWidget {
    Q_OBJECT
public:
    explicit ValueBox(const Theme* theme, QWidget* parent = nullptr);

    void setDecimals(int decimals);
    int decimals() const { return m_decimals; }
    QString text() const;
    // Starts inline text entry; Return commits, Escape cancels.
    void beginEdit();
    bool isEditing() const { return m_editing; }

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override { return sizeHint(); }

signals:
    // Emitted when a drag ends or typed text is committed, with the value
    // the user asked for (before any owner-side validation).
    void committed(double value);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void focusOutEvent(QFocusEvent* event) override;

private:
    void commitText();

    int m_decimals = 2;
    bool m_editing = false;
    QString m_buffer;
};

} // namespace nylon
