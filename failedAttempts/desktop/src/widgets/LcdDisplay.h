#pragma once

#include <QWidget>

namespace nylon {

class Theme;

// Inset display in the control bar: position, tempo, and time signature
// in a monospaced face on a dark field, with dim labels. Values come from
// the owner; nothing here animates on its own.
class LcdDisplay : public QWidget {
    Q_OBJECT
public:
    explicit LcdDisplay(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setPosition(const QString& barsBeats);
    void setTempo(double bpm);
    void setSignature(int numerator, int denominator);
    // Text shown in the key field; empty draws a dash.
    void setKey(const QString& key);
    QString position() const { return m_position; }
    QString tempoText() const;

    QSize sizeHint() const override;
    QSize minimumSizeHint() const override { return sizeHint(); }

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    const Theme* m_theme;
    QString m_position = QStringLiteral("1 . 1 . 1");
    double m_tempo = 120.0;
    int m_numerator = 4;
    int m_denominator = 4;
    QString m_key;
};

} // namespace nylon
