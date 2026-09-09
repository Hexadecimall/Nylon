// C++17 binding over the Nylon C interface. Link against nyloncpp and the
// core library. Every method maps onto one nylon_* function; the class adds
// ownership, UTF-8 string handling, and typed enums.
#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace nylon {

enum class TrackKind : int { Audio = 0, Midi = 1, Return = 2, Master = 3, Group = 4, Cue = 5 };

struct MidiNote {
    std::uint8_t pitch;
    std::uint8_t velocity;
    double startBeats;
    double lengthBeats;
};

struct BeatRange {
    double startBeats;
    double lengthBeats;
};

struct AudioDevice {
    std::uint64_t id;
    std::string name;
    std::uint32_t channels;
    bool isDefault;
    std::vector<std::uint32_t> sampleRates;
};

struct AudioConfig {
    std::uint64_t deviceId;
    std::uint32_t sampleRate;
    std::uint32_t blockFrames;
    std::uint32_t channels;
};

struct Levels {
    float peakLeft;
    float peakRight;
    float rmsLeft;
    float rmsRight;
    bool clipped;
};

struct BounceReport {
    std::uint64_t frames;
    float peakLeft;
    float peakRight;
};

struct RecordingReport {
    std::uint64_t frames;
    std::uint32_t sampleRate;
    double lengthBeats;
    std::uint64_t lostBlocks;
    std::uint64_t lostFrames;
};

enum class RoutingKind : int { Main = 0, SendPreFader = 1, SendPostFader = 2, Sidechain = 3 };

struct ProjectRoute {
    std::uint64_t source;
    std::uint64_t destination;
    RoutingKind kind;
    float gain;
};

enum class DeviceKind : int {
    Utility = 0,
    Equalizer = 1,
    Compressor = 2,
    StereoDelay = 3,
    Limiter = 4,
    Saturator = 5,
    Gate = 6
};

enum class FilterKind : int {
    LowPass = 0,
    HighPass = 1,
    BandPass = 2,
    Notch = 3,
    AllPass = 4,
    Peaking = 5,
    LowShelf = 6,
    HighShelf = 7
};

struct TrackDevice {
    DeviceKind kind{DeviceKind::Utility};
    bool enabled{true};
    std::array<float, 7> parameters{};
};

enum class AutomationParameter : int { Volume = 0, Pan = 1, Mute = 2, Solo = 3 };
enum class AutomationCurve : int { Step = 0, Linear = 1, Smooth = 2 };

struct AutomationPoint {
    double beat;
    float value;
    AutomationCurve curve;
};

class CompiledRouting {
public:
    CompiledRouting();
    ~CompiledRouting();
    CompiledRouting(const CompiledRouting&) = delete;
    CompiledRouting& operator=(const CompiledRouting&) = delete;
    CompiledRouting(CompiledRouting&& other) noexcept;
    CompiledRouting& operator=(CompiledRouting&& other) noexcept;

    explicit operator bool() const { return m_handle != nullptr; }
    std::vector<std::uint32_t> order() const;
    bool edgeDelay(std::uint32_t index, std::uint32_t& frames) const;
    bool outputLatency(std::uint32_t node, std::uint32_t& frames) const;

private:
    friend class RoutingGraph;
    explicit CompiledRouting(void* handle);
    void* m_handle{};
};

class RoutingGraph {
public:
    explicit RoutingGraph(std::uint32_t nodeCount);
    ~RoutingGraph();
    RoutingGraph(const RoutingGraph&) = delete;
    RoutingGraph& operator=(const RoutingGraph&) = delete;
    RoutingGraph(RoutingGraph&& other) noexcept;
    RoutingGraph& operator=(RoutingGraph&& other) noexcept;

    explicit operator bool() const { return m_handle != nullptr; }
    bool setNodeLatency(std::uint32_t node, std::uint32_t frames);
    bool addEdge(std::uint32_t source, std::uint32_t destination, RoutingKind kind,
        float gain, std::uint32_t& index);
    CompiledRouting compile() const;

private:
    void* m_handle{};
};

// Owning handle to a core project. Move-only.
class Project {
public:
    Project();
    ~Project();
    Project(const Project&) = delete;
    Project& operator=(const Project&) = delete;
    Project(Project&& other) noexcept;
    Project& operator=(Project&& other) noexcept;

    // False when the core failed to allocate.
    bool valid() const { return m_handle != nullptr; }
    explicit operator bool() const { return valid(); }
    void* raw() { return m_handle; }
    const void* raw() const { return m_handle; }

    // Replaces the contents with an empty project and clears history.
    bool reset();
    // Bundle directory persistence; paths are UTF-8.
    bool save(const std::string& bundleDirectory);
    bool open(const std::string& bundleDirectory);
    bool isModified() const;
    bool autosave() const;
    static bool recoveryAvailable(const std::string& bundleDirectory);
    bool recover(const std::string& bundleDirectory);
    static bool discardRecovery(const std::string& bundleDirectory);
    bool bounceWave(const std::string& path, double startBeats, double endBeats,
        std::uint32_t sampleRate, BounceReport& report) const;

    double tempo() const;
    bool setTempo(double bpm);
    int timeSignatureNumerator() const;
    int timeSignatureDenominator() const;
    bool setTimeSignature(int numerator, int denominator);
    unsigned int sampleRate() const;
    bool setSampleRate(unsigned int rate);

    bool undo();
    bool redo();
    bool canUndo() const;
    bool canRedo() const;

    std::uint64_t trackCount() const;
    bool addTrack();
    bool addTrack(TrackKind kind);
    bool deleteTrack(std::uint64_t index);

    std::string trackName(std::uint64_t index) const;
    bool setTrackName(std::uint64_t index, const std::string& utf8);
    TrackKind trackKind(std::uint64_t index) const;
    double trackVolumeDb(std::uint64_t index) const;
    bool setTrackVolumeDb(std::uint64_t index, double db);
    double trackPan(std::uint64_t index) const;
    bool setTrackPan(std::uint64_t index, double pan);
    bool trackMuted(std::uint64_t index) const;
    bool setTrackMuted(std::uint64_t index, bool on);
    bool trackSolo(std::uint64_t index) const;
    bool setTrackSolo(std::uint64_t index, bool on);
    bool trackArmed(std::uint64_t index) const;
    bool setTrackArmed(std::uint64_t index, bool on);
    int trackColorIndex(std::uint64_t index) const;
    bool setTrackColorIndex(std::uint64_t index, int color);
    std::uint32_t trackLatencyFrames(std::uint64_t index) const;
    bool setTrackLatencyFrames(std::uint64_t index, std::uint32_t frames);

    std::vector<TrackDevice> trackDevices(std::uint64_t track) const;
    bool addTrackDevice(std::uint64_t track, const TrackDevice& device);
    bool setTrackDevice(
        std::uint64_t track, std::uint64_t index, const TrackDevice& device);
    bool deleteTrackDevice(std::uint64_t track, std::uint64_t index);
    bool moveTrackDevice(std::uint64_t track, std::uint64_t from, std::uint64_t to);

    std::vector<AutomationPoint> trackAutomation(
        std::uint64_t track, AutomationParameter parameter) const;
    bool setTrackAutomation(std::uint64_t track, AutomationParameter parameter,
        const std::vector<AutomationPoint>& points);
    bool clearTrackAutomation(std::uint64_t track, AutomationParameter parameter);

    std::vector<ProjectRoute> routes() const;
    bool addRoute(std::uint64_t source, std::uint64_t destination, RoutingKind kind, float gain);
    bool deleteRoute(std::uint64_t index);

    std::uint64_t sceneCount() const;
    bool createScene(const std::string& name);
    bool deleteScene(std::uint64_t scene);
    std::string sceneName(std::uint64_t scene) const;
    bool setSceneName(std::uint64_t scene, const std::string& name);

    bool clipSlotOccupied(std::uint64_t track, std::uint64_t scene) const;
    bool createMidiClip(std::uint64_t track, std::uint64_t scene, double lengthBeats);
    bool importWave(std::uint64_t track, std::uint64_t scene, const std::string& sourcePath,
        double sourceTempo);
    bool deleteClip(std::uint64_t track, std::uint64_t scene);
    std::string clipName(std::uint64_t track, std::uint64_t scene) const;
    bool setClipName(std::uint64_t track, std::uint64_t scene, const std::string& name);
    int clipColorIndex(std::uint64_t track, std::uint64_t scene) const;
    bool setClipColorIndex(std::uint64_t track, std::uint64_t scene, int color);
    BeatRange clipLoop(std::uint64_t track, std::uint64_t scene) const;
    bool setClipLoop(std::uint64_t track, std::uint64_t scene, BeatRange range);
    std::string clipMediaPath(std::uint64_t track, std::uint64_t scene) const;
    double clipAudioGainDb(std::uint64_t track, std::uint64_t scene) const;
    bool setClipAudioGainDb(std::uint64_t track, std::uint64_t scene, double db);
    bool clipAudioReversed(std::uint64_t track, std::uint64_t scene) const;
    bool setClipAudioReversed(std::uint64_t track, std::uint64_t scene, bool enabled);
    bool clipAudioWarped(std::uint64_t track, std::uint64_t scene) const;
    double clipAudioSourceTempo(std::uint64_t track, std::uint64_t scene) const;
    bool setClipAudioWarp(
        std::uint64_t track, std::uint64_t scene, bool enabled, double sourceTempo);
    std::uint64_t clipNoteCount(std::uint64_t track, std::uint64_t scene) const;
    bool clipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index, MidiNote& note) const;
    bool addClipNote(std::uint64_t track, std::uint64_t scene, MidiNote note);
    bool removeClipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index);
    bool moveClipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index, MidiNote note);
    bool quantizeClipNotes(
        std::uint64_t track, std::uint64_t scene, double gridBeats, double strength = 1.0);
    bool transposeClipNotes(std::uint64_t track, std::uint64_t scene, int semitones);
    bool setClipNoteVelocity(std::uint64_t track, std::uint64_t scene, std::uint8_t velocity);
    bool humanizeClipNotes(std::uint64_t track, std::uint64_t scene, double timingBeats,
        std::uint8_t velocityRange, std::uint64_t seed);

    std::uint64_t arrangementClipCount(std::uint64_t track) const;
    bool addArrangementClipFromSlot(
        std::uint64_t track, std::uint64_t scene, BeatRange range);
    bool arrangementClipRange(std::uint64_t track, std::uint64_t index, BeatRange& range) const;
    std::string arrangementClipName(std::uint64_t track, std::uint64_t index) const;
    int arrangementClipColorIndex(std::uint64_t track, std::uint64_t index) const;
    bool removeArrangementClip(std::uint64_t track, std::uint64_t index);
    bool setArrangementClipRange(std::uint64_t track, std::uint64_t index, BeatRange range);

private:
    void* m_handle;
};

// Owning control-thread handle for the platform audio stream.
class AudioEngine {
public:
    AudioEngine();
    ~AudioEngine();
    AudioEngine(const AudioEngine&) = delete;
    AudioEngine& operator=(const AudioEngine&) = delete;
    AudioEngine(AudioEngine&& other) noexcept;
    AudioEngine& operator=(AudioEngine&& other) noexcept;

    bool valid() const { return m_handle != nullptr; }
    explicit operator bool() const { return valid(); }

    static std::vector<AudioDevice> devices();
    static std::vector<AudioDevice> inputDevices();
    static bool defaultOutput(std::uint64_t& deviceId);
    static bool defaultInput(std::uint64_t& deviceId);

    bool open(const Project& project, std::uint64_t deviceId, std::uint32_t sampleRate,
        std::uint32_t blockFrames);
    bool close();
    bool isOpen() const;
    bool config(AudioConfig& config) const;
    bool sync(const Project& project);
    std::uint64_t dropouts() const;
    std::uint64_t framesRendered() const;

    bool launchClip(const Project& project, std::uint64_t track, std::uint64_t scene,
        double quantizationBeats);
    bool launchScene(const Project& project, std::uint64_t scene, double quantizationBeats);
    bool stopSessionTrack(const Project& project, std::uint64_t track);
    std::int64_t activeSessionScene(std::uint64_t track) const;

    bool play();
    bool stop();
    bool locate(double beats);
    double positionBeats();
    bool isPlaying();
    bool trackLevels(std::uint64_t index, Levels& levels);
    bool masterLevels(Levels& levels);

private:
    void* m_handle;
};

// Owning control-thread handle for one input recording.
class Recording {
public:
    Recording() = default;
    ~Recording();
    Recording(const Recording&) = delete;
    Recording& operator=(const Recording&) = delete;
    Recording(Recording&& other) noexcept;
    Recording& operator=(Recording&& other) noexcept;

    bool valid() const { return m_handle != nullptr; }
    explicit operator bool() const { return valid(); }

    bool open(const Project& project, std::uint64_t track, std::uint64_t scene,
        std::uint64_t deviceId, std::uint32_t sampleRate, std::uint32_t blockFrames);
    bool start();
    bool stop();
    bool isRunning() const;
    bool finish(Project& project, RecordingReport& report);

private:
    void* m_handle{};
};

} // namespace nylon
