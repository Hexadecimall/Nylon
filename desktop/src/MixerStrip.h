#pragma once

#include <QWidget>

class QLabel;

namespace nylon {

class Fader;
class FlatButton;
class Knob;
class LevelMeter;
class Theme;

// One channel strip: name, activator/solo/arm, pan, fader with meter, and
// the volume readout. The master strip omits solo and arm.
class MixerStrip : public QWidget {
    Q_OBJECT
public:
    enum class Kind { Track, Master };

    MixerStrip(const Theme* theme, Kind kind, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);
    void setTrackIndex(int index);
    int trackIndex() const { return m_index; }
    void setName(const QString& name);
    QString name() const;
    void setColor(const QColor& color);
    void setSelected(bool selected);
    bool isSelected() const { return m_selected; }
    // Enables the controls that write to the core. They stay disabled until
    // the core exposes mixer state for the track.
    void setInteractive(bool interactive);
    bool isInteractive() const { return m_interactive; }
    // Updates every control from core state without emitting edit signals.
    void setState(double volumeDb, double pan, bool active, bool solo, bool armed);

    Fader* fader() const { return m_fader; }
    Knob* pan() const { return m_pan; }
    LevelMeter* meter() const { return m_meter; }
    FlatButton* activator() const { return m_activator; }
    FlatButton* soloButton() const { return m_solo; }
    FlatButton* armButton() const { return m_arm; }

    QSize sizeHint() const override;

signals:
    void selected(int index);
    // Volume and pan are reported when a drag or edit finishes, so one
    // gesture becomes one undo step.
    void volumeChanged(int index, double db);
    void panChanged(int index, double pan);
    void activeToggled(int index, bool active);
    void soloToggled(int index, bool solo);
    void armToggled(int index, bool armed);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;

private:
    const Theme* m_theme;
    Kind m_kind;
    int m_index = -1;
    bool m_selected = false;
    bool m_interactive = false;
    QColor m_color;
    QLabel* m_name;
    FlatButton* m_activator;
    FlatButton* m_solo;
    FlatButton* m_arm;
    Knob* m_pan;
    Fader* m_fader;
    LevelMeter* m_meter;
    QLabel* m_volume;
    bool m_syncing = false;
};

} // namespace nylon
