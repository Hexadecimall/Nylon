// C++17 binding over the Nylon C interface. Link against nyloncpp and the
// core library. Every method maps onto one nylon_* function; the class adds
// ownership, UTF-8 string handling, and typed enums.
#pragma once

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
    bool save(const std::string& bundleDirectory) const;
    bool open(const std::string& bundleDirectory);

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

    std::uint64_t sceneCount() const;
    bool createScene(const std::string& name);
    bool deleteScene(std::uint64_t scene);
    std::string sceneName(std::uint64_t scene) const;
    bool setSceneName(std::uint64_t scene, const std::string& name);

    bool clipSlotOccupied(std::uint64_t track, std::uint64_t scene) const;
    bool createMidiClip(std::uint64_t track, std::uint64_t scene, double lengthBeats);
    bool deleteClip(std::uint64_t track, std::uint64_t scene);
    std::string clipName(std::uint64_t track, std::uint64_t scene) const;
    bool setClipName(std::uint64_t track, std::uint64_t scene, const std::string& name);
    int clipColorIndex(std::uint64_t track, std::uint64_t scene) const;
    bool setClipColorIndex(std::uint64_t track, std::uint64_t scene, int color);
    BeatRange clipLoop(std::uint64_t track, std::uint64_t scene) const;
    bool setClipLoop(std::uint64_t track, std::uint64_t scene, BeatRange range);
    std::uint64_t clipNoteCount(std::uint64_t track, std::uint64_t scene) const;
    bool clipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index, MidiNote& note) const;
    bool addClipNote(std::uint64_t track, std::uint64_t scene, MidiNote note);
    bool removeClipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index);
    bool moveClipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index, MidiNote note);

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
    static bool defaultOutput(std::uint64_t& deviceId);

    bool open(const Project& project, std::uint64_t deviceId, std::uint32_t sampleRate,
        std::uint32_t blockFrames);
    bool close();
    bool isOpen() const;
    bool config(AudioConfig& config) const;
    bool sync(const Project& project);
    std::uint64_t dropouts() const;

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

} // namespace nylon
