// C++17 binding over the Nylon C interface. Link against nyloncpp and the
// core library. Every method maps onto one nylon_* function; the class adds
// ownership, UTF-8 string handling, and typed enums.
#pragma once

#include <cstdint>
#include <string>

namespace nylon {

enum class TrackKind : int { Audio = 0, Midi = 1, Return = 2, Master = 3, Group = 4, Cue = 5 };

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

private:
    void* m_handle;
};

} // namespace nylon
