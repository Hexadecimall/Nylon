#pragma once

#include <QWidget>

class QDoubleSpinBox;
class QPushButton;
class QToolButton;
class QLabel;
class QSpacerItem;

namespace nylon {

class ProjectBridge;
class Theme;

// Top strip: tempo entry, track/undo/redo controls, and the view switch.
// There is no play control because the core exposes no transport yet.
class TransportBar : public QWidget {
    Q_OBJECT
public:
    explicit TransportBar(ProjectBridge* bridge, QWidget* parent = nullptr);

    void applyTheme(const Theme& theme);

    QDoubleSpinBox* tempoBox() const { return m_tempo; }
    QPushButton* addTrackButton() const { return m_addTrack; }
    QPushButton* undoButton() const { return m_undo; }
    QPushButton* redoButton() const { return m_redo; }
    QToolButton* sessionButton() const { return m_session; }
    QToolButton* arrangementButton() const { return m_arrangement; }

signals:
    // Emitted after the core rejected an edit or reported nothing to do.
    void message(const QString& text);
    void sessionRequested();
    void arrangementRequested();

public slots:
    void showSessionActive(bool session);

private:
    void refresh();
    void commitTempo();

    ProjectBridge* m_bridge;
    QDoubleSpinBox* m_tempo;
    QPushButton* m_addTrack;
    QPushButton* m_undo;
    QPushButton* m_redo;
    QToolButton* m_session;
    QToolButton* m_arrangement;
    QLabel* m_trackCount;
    QSpacerItem* m_gapA = nullptr;
    QSpacerItem* m_gapB = nullptr;
};

} // namespace nylon
