#pragma once

#include "nylon.hpp"

#include <QObject>

namespace nylon {

// Owns one core project handle and forwards edits to it. Every mutating
// call emits changed() when the core accepted it so views can repaint from
// the new snapshot.
class ProjectBridge : public QObject {
    Q_OBJECT
public:
    explicit ProjectBridge(QObject* parent = nullptr);
    ~ProjectBridge() override;

    ProjectBridge(const ProjectBridge&) = delete;
    ProjectBridge& operator=(const ProjectBridge&) = delete;

    // False when the core failed to allocate a project.
    bool isValid() const { return m_project.valid(); }
    // The underlying binding, for code that needs the raw handle.
    Project& project() { return m_project; }
    const Project& project() const { return m_project; }

    using TrackKind = nylon::TrackKind;

    double tempo() const;
    quint64 trackCount() const;
    bool canUndo() const;
    bool canRedo() const;
    int timeSignatureNumerator() const;
    int timeSignatureDenominator() const;
    unsigned int sampleRate() const;

    QString trackName(quint64 index) const;
    TrackKind trackKind(quint64 index) const;
    static QString kindName(TrackKind kind);
    double trackVolumeDb(quint64 index) const;
    double trackPan(quint64 index) const;
    bool trackMuted(quint64 index) const;
    bool trackSolo(quint64 index) const;
    bool trackArmed(quint64 index) const;
    int trackColorIndex(quint64 index) const;

    // True once the core can read and write project bundles.
    static bool isPersistenceAvailable() { return false; }
    // True once the core drives a transport from an audio backend.
    static bool isTransportAvailable() { return false; }
    // True once the core exposes per-track mixer state.
    static bool isMixerAvailable() { return true; }

public slots:
    // Discards the current project and starts an empty one.
    bool reset();
    bool setTempo(double bpm);
    bool setTimeSignature(int numerator, int denominator);
    bool setSampleRate(unsigned int rate);
    bool addTrack();
    bool addTrack(TrackKind kind);
    bool deleteTrack(quint64 index);
    bool setTrackName(quint64 index, const QString& name);
    bool setTrackVolumeDb(quint64 index, double db);
    bool setTrackPan(quint64 index, double pan);
    bool setTrackMuted(quint64 index, bool muted);
    bool setTrackSolo(quint64 index, bool solo);
    bool setTrackArmed(quint64 index, bool armed);
    bool setTrackColorIndex(quint64 index, int color);
    bool undo();
    bool redo();

signals:
    void changed();

private:
    Project m_project;
};

} // namespace nylon
