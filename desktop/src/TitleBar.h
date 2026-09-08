#pragma once

#include <QWidget>

class QLabel;
class QMenuBar;

namespace nylon {

class Theme;

// Custom title bar for the frameless main window: window controls on the
// left, the menu bar next to them, and the document title centered.
// Dragging moves the window; double-clicking toggles maximize.
class TitleBar : public QWidget {
    Q_OBJECT
public:
    explicit TitleBar(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setTitle(const QString& title);
    QString title() const;
    // Menu bar owned by the title bar; the window builds its menus here.
    QMenuBar* menuBar() const { return m_menuBar; }

    // Rectangles of the three window controls in widget coordinates.
    QRect closeRect() const;
    QRect minimizeRect() const;
    QRect zoomRect() const;

    QSize sizeHint() const override;

signals:
    void closeRequested();
    void minimizeRequested();
    void zoomRequested();

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void leaveEvent(QEvent* event) override;

private:
    QRect controlRect(int index) const;
    int controlAt(const QPoint& pos) const;

    const Theme* m_theme;
    QMenuBar* m_menuBar;
    QLabel* m_title;
    int m_hoverControl = -1;
    int m_pressedControl = -1;
};

} // namespace nylon
