#include "nylon.hpp"

#include "nylon.h"

#include <utility>

namespace nylon {

Project::Project()
    : m_handle(nylon_project_new())
{
}

Project::~Project()
{
    nylon_project_free(m_handle);
}

Project::Project(Project&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}

Project& Project::operator=(Project&& other) noexcept
{
    if (this != &other) {
        nylon_project_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

bool Project::reset() { return nylon_project_new_in_place(m_handle) != 0; }
bool Project::save(const std::string& bundleDirectory) const
{
    return nylon_project_save(m_handle, bundleDirectory.c_str()) != 0;
}
bool Project::open(const std::string& bundleDirectory)
{
    return nylon_project_open(m_handle, bundleDirectory.c_str()) != 0;
}

double Project::tempo() const { return nylon_project_tempo(m_handle); }
bool Project::setTempo(double bpm) { return nylon_project_set_tempo(m_handle, bpm) != 0; }
int Project::timeSignatureNumerator() const { return nylon_project_time_signature_numerator(m_handle); }
int Project::timeSignatureDenominator() const { return nylon_project_time_signature_denominator(m_handle); }
bool Project::setTimeSignature(int numerator, int denominator)
{
    return nylon_project_set_time_signature(m_handle, numerator, denominator) != 0;
}
unsigned int Project::sampleRate() const { return nylon_project_sample_rate(m_handle); }
bool Project::setSampleRate(unsigned int rate) { return nylon_project_set_sample_rate(m_handle, rate) != 0; }

bool Project::undo() { return nylon_project_undo(m_handle) != 0; }
bool Project::redo() { return nylon_project_redo(m_handle) != 0; }
bool Project::canUndo() const { return nylon_project_can_undo(m_handle) != 0; }
bool Project::canRedo() const { return nylon_project_can_redo(m_handle) != 0; }

std::uint64_t Project::trackCount() const { return nylon_project_track_count(m_handle); }
bool Project::addTrack() { return nylon_project_add_track(m_handle) != 0; }
bool Project::addTrack(TrackKind kind) { return nylon_project_add_track_kind(m_handle, static_cast<int>(kind)) != 0; }
bool Project::deleteTrack(std::uint64_t index) { return nylon_track_delete(m_handle, index) != 0; }

std::string Project::trackName(std::uint64_t index) const
{
    char small[128];
    const auto needed = nylon_track_name(m_handle, index, small, sizeof(small));
    if (needed < sizeof(small)) {
        return std::string(small);
    }
    std::string large(static_cast<std::size_t>(needed), '\0');
    nylon_track_name(m_handle, index, large.data(), needed + 1);
    return large;
}

bool Project::setTrackName(std::uint64_t index, const std::string& utf8)
{
    return nylon_track_set_name(m_handle, index, utf8.c_str()) != 0;
}

TrackKind Project::trackKind(std::uint64_t index) const
{
    const int k = nylon_track_kind(m_handle, index);
    return (k >= 0 && k <= 5) ? static_cast<TrackKind>(k) : TrackKind::Audio;
}

double Project::trackVolumeDb(std::uint64_t index) const { return nylon_track_volume_db(m_handle, index); }
bool Project::setTrackVolumeDb(std::uint64_t index, double db) { return nylon_track_set_volume_db(m_handle, index, db) != 0; }
double Project::trackPan(std::uint64_t index) const { return nylon_track_pan(m_handle, index); }
bool Project::setTrackPan(std::uint64_t index, double pan) { return nylon_track_set_pan(m_handle, index, pan) != 0; }
bool Project::trackMuted(std::uint64_t index) const { return nylon_track_mute(m_handle, index) != 0; }
bool Project::setTrackMuted(std::uint64_t index, bool on) { return nylon_track_set_mute(m_handle, index, on ? 1 : 0) != 0; }
bool Project::trackSolo(std::uint64_t index) const { return nylon_track_solo(m_handle, index) != 0; }
bool Project::setTrackSolo(std::uint64_t index, bool on) { return nylon_track_set_solo(m_handle, index, on ? 1 : 0) != 0; }
bool Project::trackArmed(std::uint64_t index) const { return nylon_track_arm(m_handle, index) != 0; }
bool Project::setTrackArmed(std::uint64_t index, bool on) { return nylon_track_set_arm(m_handle, index, on ? 1 : 0) != 0; }
int Project::trackColorIndex(std::uint64_t index) const { return nylon_track_color_index(m_handle, index); }
bool Project::setTrackColorIndex(std::uint64_t index, int color)
{
    return nylon_track_set_color_index(m_handle, index, color) != 0;
}

} // namespace nylon
