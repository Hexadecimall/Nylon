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
    Gate = 6,
    Chorus = 7,
    Reverb = 8,
    AutoFilter = 9,
    Phaser = 10
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
    std::array<float, 16> parameters{};
};

enum class AutomationParameter : int { Volume = 0, Pan = 1, Mute = 2, Solo = 3 };
enum class AutomationCurve : int { Step = 0, Linear = 1, Smooth = 2 };

struct AutomationPoint {
    double beat;
    float value;
    AutomationCurve curve;
};

enum class OscillatorShape : int { Sine = 0, Saw = 1, Square = 2, Triangle = 3 };

struct InstrumentPatch {
    OscillatorShape shapeA{OscillatorShape::Saw};
    OscillatorShape shapeB{OscillatorShape::Square};
    float oscillatorMix{};
    float oscillatorBDetuneCents{7.0F};
    float subLevel{};
    float noiseLevel{};
    std::uint32_t unisonVoices{1};
    float unisonDetuneCents{12.0F};
    float attackSeconds{0.004F};
    float decaySeconds{0.12F};
    float sustain{0.65F};
    float releaseSeconds{0.18F};
    float cutoffHz{4'000.0F};
    float resonance{0.9F};
    float levelDb{-12.0F};
};

enum class PluginFormat : int { Vst3 = 0, AudioUnit = 1, Clap = 2, Lv2 = 3 };
enum class PluginState : int { Discovered = 0, Quarantined = 1 };

struct PluginParameterValue {
    std::uint32_t identifier{};
    double value{};
};

struct TrackPluginDevice {
    PluginFormat format{PluginFormat::Clap};
    bool enabled{true};
    std::string package;
    std::string identifier;
    std::uint32_t latencyFrames{};
    std::vector<std::uint8_t> state;
    std::vector<PluginParameterValue> parameters;
};

struct PluginDescriptor {
    std::string id;
    std::string name;
    std::string vendor;
    std::string version;
    std::vector<std::string> features;
};

struct PluginInfo {
    std::string path;
    std::string name;
    PluginFormat format;
    PluginState state;
    std::string quarantineReason;
    std::vector<PluginDescriptor> descriptors;
};

struct PluginScanIssue {
    std::string path;
    std::string message;
};

class PluginCatalog {
public:
    PluginCatalog() = default;
    ~PluginCatalog();
    PluginCatalog(const PluginCatalog&) = delete;
    PluginCatalog& operator=(const PluginCatalog&) = delete;
    PluginCatalog(PluginCatalog&& other) noexcept;
    PluginCatalog& operator=(PluginCatalog&& other) noexcept;

    static PluginCatalog scan(const std::vector<std::string>& roots);
    bool valid() const { return m_handle != nullptr; }
    explicit operator bool() const { return valid(); }
    std::vector<PluginInfo> entries() const;
    std::vector<PluginScanIssue> issues() const;
    bool applyProbe(std::uint64_t index, const std::vector<std::uint8_t>& bytes);
    bool quarantine(std::uint64_t index, const std::string& reason);
    bool retry(std::uint64_t index);

private:
    explicit PluginCatalog(void* handle)
        : m_handle(handle)
    {
    }
    void* m_handle{};
};

class ClapInstance {
public:
    ClapInstance() = default;
    ~ClapInstance();
    ClapInstance(const ClapInstance&) = delete;
    ClapInstance& operator=(const ClapInstance&) = delete;
    ClapInstance(ClapInstance&& other) noexcept;
    ClapInstance& operator=(ClapInstance&& other) noexcept;

    static ClapInstance open(const std::string& path, const std::string& identifier);
    explicit operator bool() const { return m_handle != nullptr; }
    bool activate(double sampleRate, std::uint32_t minFrames, std::uint32_t maxFrames);
    bool processStereo(const float* inputLeft, const float* inputRight,
        float* outputLeft, float* outputRight, std::uint32_t frames);
    struct ParameterEvent {
        std::uint32_t sampleOffset;
        std::uint32_t identifier;
        double value;
    };
    struct ParameterInfo {
        std::uint32_t identifier;
        std::uint32_t flags;
        std::string name;
        std::string module;
        double minimum;
        double maximum;
        double defaultValue;
    };
    struct NoteEvent {
        std::uint32_t sampleOffset;
        std::uint32_t kind;
        std::int32_t noteId;
        std::int16_t portIndex;
        std::int16_t channel;
        std::int16_t key;
        double velocity;
    };
    bool processStereo(const float* inputLeft, const float* inputRight,
        float* outputLeft, float* outputRight, std::uint32_t frames,
        const ParameterEvent* events, std::uint32_t eventCount);
    bool processStereo(const float* inputLeft, const float* inputRight,
        float* outputLeft, float* outputRight, std::uint32_t frames,
        const ParameterEvent* parameterEvents, std::uint32_t parameterEventCount,
        const NoteEvent* noteEvents, std::uint32_t noteEventCount);
    std::uint32_t inputNotePorts() const;
    std::uint32_t inputAudioPorts() const;
    std::vector<ParameterInfo> parameters() const;
    bool parameterValue(std::uint32_t identifier, double& value) const;
    bool latency(std::uint32_t& frames) const;
    bool saveState(std::vector<std::uint8_t>& state) const;
    bool loadState(const std::vector<std::uint8_t>& state);
    bool reset();
    std::uint32_t takeRequests();

private:
    explicit ClapInstance(void* handle)
        : m_handle(handle)
    {
    }
    void* m_handle{};
};

class ClapWorker {
public:
    using ParameterEvent = ClapInstance::ParameterEvent;
    using ParameterInfo = ClapInstance::ParameterInfo;
    using NoteEvent = ClapInstance::NoteEvent;

    ClapWorker() = default;
    ~ClapWorker();
    ClapWorker(const ClapWorker&) = delete;
    ClapWorker& operator=(const ClapWorker&) = delete;
    ClapWorker(ClapWorker&& other) noexcept;
    ClapWorker& operator=(ClapWorker&& other) noexcept;

    static ClapWorker open(const std::string& executable, const std::string& path,
        const std::string& identifier, double sampleRate, std::uint32_t maxFrames);
    explicit operator bool() const { return m_handle != nullptr; }
    bool processStereo(const float* inputLeft, const float* inputRight,
        float* outputLeft, float* outputRight, std::uint32_t frames,
        const ParameterEvent* parameterEvents, std::uint32_t parameterEventCount,
        const NoteEvent* noteEvents, std::uint32_t noteEventCount);
    std::uint32_t inputNotePorts() const;
    std::uint32_t inputAudioPorts() const;
    std::vector<ParameterInfo> parameters() const;
    bool latency(std::uint32_t& frames) const;
    bool saveState(std::vector<std::uint8_t>& state);
    bool loadState(const std::vector<std::uint8_t>& state);

private:
    explicit ClapWorker(void* handle)
        : m_handle(handle)
    {
    }
    void* m_handle{};
};

class ClapBridge {
public:
    using ParameterEvent = ClapInstance::ParameterEvent;
    using NoteEvent = ClapInstance::NoteEvent;

    ClapBridge() = default;
    ~ClapBridge();
    ClapBridge(const ClapBridge&) = delete;
    ClapBridge& operator=(const ClapBridge&) = delete;
    ClapBridge(ClapBridge&& other) noexcept;
    ClapBridge& operator=(ClapBridge&& other) noexcept;

    static ClapBridge open(const std::string& executable, const std::string& path,
        const std::string& identifier, double sampleRate, std::uint32_t frames,
        std::uint32_t queueDepth);
    explicit operator bool() const { return m_handle != nullptr; }
    bool processStereo(const float* inputLeft, const float* inputRight,
        float* outputLeft, float* outputRight, std::uint32_t frames,
        const ParameterEvent* parameterEvents, std::uint32_t parameterEventCount,
        const NoteEvent* noteEvents, std::uint32_t noteEventCount);
    std::uint32_t latency() const;
    bool isRunning() const;
    std::uint64_t submittedBlocks() const;
    std::uint64_t completedBlocks() const;
    std::uint64_t underruns() const;
    std::uint64_t queueDrops() const;
    std::uint64_t workerFailures() const;

private:
    explicit ClapBridge(void* handle)
        : m_handle(handle)
    {
    }
    void* m_handle{};
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
    bool trackInstrument(std::uint64_t track, InstrumentPatch& patch) const;
    bool setTrackInstrument(std::uint64_t track, const InstrumentPatch& patch);

    std::vector<TrackDevice> trackDevices(std::uint64_t track) const;
    std::uint64_t trackDeviceCount(std::uint64_t track) const;
    int trackDeviceType(std::uint64_t track, std::uint64_t index) const;
    bool trackDevice(std::uint64_t track, std::uint64_t index, TrackDevice& device) const;
    bool trackPluginDevice(
        std::uint64_t track, std::uint64_t index, TrackPluginDevice& device) const;
    bool addTrackDevice(std::uint64_t track, const TrackDevice& device);
    bool addTrackPlugin(std::uint64_t track, const TrackPluginDevice& device);
    bool setTrackDevice(
        std::uint64_t track, std::uint64_t index, const TrackDevice& device);
    bool setTrackDeviceEnabled(std::uint64_t track, std::uint64_t index, bool enabled);
    bool setTrackPluginState(std::uint64_t track, std::uint64_t index,
        const std::vector<std::uint8_t>& state);
    bool setTrackPluginParameter(std::uint64_t track, std::uint64_t index,
        std::uint32_t identifier, double value);
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

    bool configurePluginHost(
        const std::string& worker, const std::vector<std::string>& roots);
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
    bool noteOn(const Project& project, std::uint64_t track, std::uint8_t pitch,
        std::uint8_t velocity);
    bool noteOff(const Project& project, std::uint64_t track, std::uint8_t pitch);
    bool allNotesOff(const Project& project, std::uint64_t track);

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
