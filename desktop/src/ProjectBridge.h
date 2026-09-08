#pragma once

#include "nylon.hpp"

#include <QList>
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
    quint64 sceneCount() const;
    QString sceneName(quint64 scene) const;
    bool clipSlotOccupied(quint64 track, quint64 scene) const;
    QString clipName(quint64 track, quint64 scene) const;
    int clipColorIndex(quint64 track, quint64 scene) const;
    BeatRange clipLoop(quint64 track, quint64 scene) const;
    quint64 clipNoteCount(quint64 track, quint64 scene) const;
    bool clipNote(quint64 track, quint64 scene, quint64 index, MidiNote& note) const;
    quint64 arrangementClipCount(quint64 track) const;
    bool arrangementClipRange(quint64 track, quint64 index, BeatRange& range) const;
    QString arrangementClipName(quint64 track, quint64 index) const;
    int arrangementClipColorIndex(quint64 track, quint64 index) const;

    // True once the core can read and write project bundles.
    static bool isPersistenceAvailable() { return true; }
    // Bundle directory of the last successful save or open; empty for an
    // unsaved project.
    QString bundlePath() const { return m_bundlePath; }
    // True once an output is open and the core is driving a transport.
    bool isTransportAvailable() const { return m_audio.isOpen(); }
    // Output devices the core found, and the one currently open.
    static QList<AudioDevice> audioDevices();
    // True when the machine has an output the engine could open. This asks
    // the core about devices; it does not start a stream.
    static bool hasAudioOutput();
    bool isAudioOpen() const { return m_audio.isOpen(); }
    QString audioDeviceName() const { return m_deviceName; }
    // Frames the device asked for that arrived late enough to be dropped.
    quint64 audioDropouts() const;
    bool isPlaying() const;
    double positionBeats() const;
    // Peak and RMS for one track, or for the master when the index is past
    // the last track. False when nothing is playing.
    bool trackLevels(quint64 index, Levels& levels) const;
    bool masterLevels(Levels& levels) const;
    // True once the core exposes per-track mixer state.
    static bool isMixerAvailable() { return true; }

public slots:
    // Opens the default output, or the named device when one is given.
    // The project is handed to the engine as it stands.
    bool openAudio(quint64 deviceId = 0, unsigned int sampleRate = 0, unsigned int blockFrames = 0);
    bool closeAudio();
    bool play();
    bool stop();
    bool locate(double beats);

    // Discards the current project and starts an empty one.
    bool reset();
    // Bundle directory persistence. Both update bundlePath() on success.
    bool save(const QString& bundleDirectory);
    bool open(const QString& bundleDirectory);
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
    bool createScene(const QString& name);
    bool deleteScene(quint64 scene);
    bool setSceneName(quint64 scene, const QString& name);
    bool createMidiClip(quint64 track, quint64 scene, double lengthBeats);
    bool deleteClip(quint64 track, quint64 scene);
    bool setClipName(quint64 track, quint64 scene, const QString& name);
    bool setClipColorIndex(quint64 track, quint64 scene, int color);
    bool setClipLoop(quint64 track, quint64 scene, BeatRange range);
    bool addClipNote(quint64 track, quint64 scene, MidiNote note);
    bool removeClipNote(quint64 track, quint64 scene, quint64 index);
    bool moveClipNote(quint64 track, quint64 scene, quint64 index, MidiNote note);
    bool addArrangementClipFromSlot(quint64 track, quint64 scene, BeatRange range);
    bool removeArrangementClip(quint64 track, quint64 index);
    bool setArrangementClipRange(quint64 track, quint64 index, BeatRange range);
    bool undo();
    bool redo();

signals:
    void changed();
    // The engine opened, closed, started or stopped.
    void audioStateChanged();

private:
    // Hands the current project to a running engine. Called after every
    // accepted edit so playback follows what is on screen.
    void syncAudio();

    Project m_project;
    QString m_bundlePath;
    mutable AudioEngine m_audio;
    QString m_deviceName;
};

} // namespace nylon
