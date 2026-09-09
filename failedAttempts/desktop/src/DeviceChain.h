#pragma once

#include <QColor>
#include <QString>
#include <QWidget>

namespace nylon {

class Theme;

// The device chain of one track. Devices are not loadable yet, so this
// draws the slots a chain is made of and says what belongs in each. It is
// the shape the chain keeps once devices exist.
class DeviceChain : public QWidget {
    Q_OBJECT
public:
    explicit DeviceChain(const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    // The track the chain belongs to. An empty name clears it.
    void setTrack(const QString& name, const QColor& color, bool instrumentSlot);

    QString trackName() const { return m_name; }
    bool hasInstrumentSlot() const { return m_instrument; }
    // Rectangle of one slot, for hit testing and for tests.
    QRect slotRect(int index) const;
    int slotCount() const;

    QSize sizeHint() const override;

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    const Theme* m_theme;
    QString m_name;
    QColor m_color;
    bool m_instrument = false;
};

} // namespace nylon
