#pragma once

#include <QAbstractButton>

namespace nylon {

class Theme;

// Flat rectangular button painted from theme tokens. When checked it fills
// with the color named by `activeColorKey` (track activator, solo, arm,
// play, record all differ) and draws its text in `accent.text`.
class FlatButton : public QAbstractButton {
    Q_OBJECT
public:
    // Icon shapes for transport-style buttons drawn without glyph fonts.
    enum class Glyph { None, Play, Stop, Record, Loop, Metronome, Circle, Square, Triangle };

    explicit FlatButton(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setActiveColorKey(const QString& key);
    QString activeColorKey() const { return m_activeKey; }
    void setGlyph(Glyph glyph);
    Glyph glyph() const { return m_glyph; }
    // Fixed size for square controls; zero keeps the size hint from text.
    void setSquare(int side);

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override { return sizeHint(); }

protected:
    void paintEvent(QPaintEvent* event) override;
    void enterEvent(QEnterEvent* event) override;
    void leaveEvent(QEvent* event) override;

private:
    void paintGlyph(QPainter& p, const QRect& r, const QColor& color) const;

    const Theme* m_theme;
    QString m_activeKey = QStringLiteral("accent");
    Glyph m_glyph = Glyph::None;
    int m_square = 0;
    bool m_hover = false;
};

} // namespace nylon
