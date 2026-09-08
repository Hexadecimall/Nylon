#pragma once

#include <QWidget>

class QLabel;
class QSpacerItem;

namespace nylon {

class FlatButton;
class Knob;
class LcdDisplay;
class ProjectBridge;
class Theme;
class ValueBox;

// Top strip laid out like a hardware transport: tempo section on the
// left, transport controls in the middle, view switch on the right.
// Playback controls exist but stay disabled until an audio backend drives
// the transport; they never pretend to run.
class TransportBar : public QWidget {
    Q_OBJECT
public:
    explicit TransportBar(ProjectBridge* bridge, const Theme* theme, QWidget* parent = nullptr);

    void setTheme(const Theme* theme);

    ValueBox* tempoBox() const { return m_tempo; }
    FlatButton* tapButton() const { return m_tap; }
    FlatButton* metronomeButton() const { return m_metronome; }
    FlatButton* playButton() const { return m_play; }
    FlatButton* stopButton() const { return m_stop; }
    FlatButton* recordButton() const { return m_record; }
    FlatButton* loopButton() const { return m_loop; }
    FlatButton* sessionButton() const { return m_session; }
    FlatButton* arrangementButton() const { return m_arrangement; }
    LcdDisplay* lcd() const { return m_lcd; }
    ValueBox* numeratorBox() const { return m_numerator; }
    ValueBox* denominatorBox() const { return m_denominator; }
    bool isTransportAvailable() const { return m_transportAvailable; }

protected:
    void paintEvent(QPaintEvent* event) override;

signals:
    void message(const QString& text);
    void sessionRequested();
    void arrangementRequested();
    void playRequested();
    void stopRequested();

public slots:
    void showSessionActive(bool session);
    // Shows the playhead in bars, beats and sixteenths.
    void showPosition(double beats);
    // Called once the core reports a running transport; enables the
    // playback controls.
    void setTransportAvailable(bool available);

private:
    void refresh();
    void commitTempo(double bpm);

    ProjectBridge* m_bridge;
    const Theme* m_theme;
    ValueBox* m_tempo;
    FlatButton* m_tap;
    ValueBox* m_numerator;
    ValueBox* m_denominator;
    FlatButton* m_metronome;
    LcdDisplay* m_lcd;
    FlatButton* m_play;
    FlatButton* m_stop;
    FlatButton* m_record;
    FlatButton* m_loop;
    QLabel* m_trackCount;
    FlatButton* m_session;
    FlatButton* m_arrangement;
    bool m_transportAvailable = false;
};

} // namespace nylon
