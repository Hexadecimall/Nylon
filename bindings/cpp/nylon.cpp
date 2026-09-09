#include "nylon.hpp"

#include "nylon.h"

#include <utility>

namespace nylon {

namespace {
using CatalogTextGetter = decltype(&nylon_plugin_catalog_entry_path);
using DescriptorTextGetter = decltype(&nylon_plugin_catalog_descriptor_id);

std::string catalogText(
    const void* catalog, std::uint64_t index, CatalogTextGetter getter)
{
    const auto needed = getter(catalog, index, nullptr, 0);
    std::vector<char> bytes(static_cast<std::size_t>(needed) + 1U, '\0');
    getter(catalog, index, bytes.data(), static_cast<unsigned long long>(bytes.size()));
    return std::string(bytes.data(), static_cast<std::size_t>(needed));
}

std::string descriptorText(const void* catalog, std::uint64_t entry,
    std::uint64_t descriptor, DescriptorTextGetter getter)
{
    const auto needed = getter(catalog, entry, descriptor, nullptr, 0);
    std::vector<char> bytes(static_cast<std::size_t>(needed) + 1U, '\0');
    getter(catalog, entry, descriptor, bytes.data(),
        static_cast<unsigned long long>(bytes.size()));
    return std::string(bytes.data(), static_cast<std::size_t>(needed));
}

std::string descriptorFeature(const void* catalog, std::uint64_t entry,
    std::uint64_t descriptor, std::uint64_t feature)
{
    const auto needed = nylon_plugin_catalog_descriptor_feature(
        catalog, entry, descriptor, feature, nullptr, 0);
    std::vector<char> bytes(static_cast<std::size_t>(needed) + 1U, '\0');
    nylon_plugin_catalog_descriptor_feature(catalog, entry, descriptor, feature,
        bytes.data(), static_cast<unsigned long long>(bytes.size()));
    return std::string(bytes.data(), static_cast<std::size_t>(needed));
}
} // namespace

PluginCatalog::~PluginCatalog() { nylon_plugin_catalog_free(m_handle); }
PluginCatalog::PluginCatalog(PluginCatalog&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}
PluginCatalog& PluginCatalog::operator=(PluginCatalog&& other) noexcept
{
    if (this != &other) {
        nylon_plugin_catalog_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

PluginCatalog PluginCatalog::scan(const std::vector<std::string>& roots)
{
    std::vector<const char*> nativeRoots;
    nativeRoots.reserve(roots.size());
    for (const auto& root : roots) nativeRoots.push_back(root.c_str());
    return PluginCatalog(nylon_plugin_catalog_scan(
        nativeRoots.empty() ? nullptr : nativeRoots.data(),
        static_cast<unsigned long long>(nativeRoots.size())));
}

std::vector<PluginInfo> PluginCatalog::entries() const
{
    const auto count = nylon_plugin_catalog_entry_count(m_handle);
    std::vector<PluginInfo> result;
    result.reserve(static_cast<std::size_t>(count));
    for (std::uint64_t index = 0; index < count; ++index) {
        const int format = nylon_plugin_catalog_entry_format(m_handle, index);
        const int state = nylon_plugin_catalog_entry_state(m_handle, index);
        if (format < 0 || state < 0) return {};
        std::vector<PluginDescriptor> descriptors;
        const auto descriptorCount = nylon_plugin_catalog_descriptor_count(m_handle, index);
        descriptors.reserve(static_cast<std::size_t>(descriptorCount));
        for (std::uint64_t descriptor = 0; descriptor < descriptorCount; ++descriptor) {
            std::vector<std::string> features;
            const auto featureCount = nylon_plugin_catalog_descriptor_feature_count(
                m_handle, index, descriptor);
            features.reserve(static_cast<std::size_t>(featureCount));
            for (std::uint64_t feature = 0; feature < featureCount; ++feature)
                features.push_back(descriptorFeature(m_handle, index, descriptor, feature));
            descriptors.push_back({descriptorText(m_handle, index, descriptor,
                                       nylon_plugin_catalog_descriptor_id),
                descriptorText(m_handle, index, descriptor,
                    nylon_plugin_catalog_descriptor_name),
                descriptorText(m_handle, index, descriptor,
                    nylon_plugin_catalog_descriptor_vendor),
                descriptorText(m_handle, index, descriptor,
                    nylon_plugin_catalog_descriptor_version),
                std::move(features)});
        }
        result.push_back({catalogText(m_handle, index, nylon_plugin_catalog_entry_path),
            catalogText(m_handle, index, nylon_plugin_catalog_entry_name),
            static_cast<PluginFormat>(format), static_cast<PluginState>(state),
            catalogText(m_handle, index, nylon_plugin_catalog_entry_reason),
            std::move(descriptors)});
    }
    return result;
}

std::vector<PluginScanIssue> PluginCatalog::issues() const
{
    const auto count = nylon_plugin_catalog_issue_count(m_handle);
    std::vector<PluginScanIssue> result;
    result.reserve(static_cast<std::size_t>(count));
    for (std::uint64_t index = 0; index < count; ++index) {
        result.push_back({catalogText(m_handle, index, nylon_plugin_catalog_issue_path),
            catalogText(m_handle, index, nylon_plugin_catalog_issue_message)});
    }
    return result;
}

bool PluginCatalog::quarantine(std::uint64_t index, const std::string& reason)
{
    return nylon_plugin_catalog_quarantine(m_handle, index, reason.c_str()) != 0;
}

bool PluginCatalog::retry(std::uint64_t index)
{
    return nylon_plugin_catalog_retry(m_handle, index) != 0;
}

bool PluginCatalog::applyProbe(std::uint64_t index, const std::vector<std::uint8_t>& bytes)
{
    return nylon_plugin_catalog_apply_probe(m_handle, index,
               bytes.empty() ? nullptr : bytes.data(),
               static_cast<unsigned long long>(bytes.size()))
        != 0;
}

ClapInstance::~ClapInstance() { nylon_clap_instance_free(m_handle); }
ClapInstance::ClapInstance(ClapInstance&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}
ClapInstance& ClapInstance::operator=(ClapInstance&& other) noexcept
{
    if (this != &other) {
        nylon_clap_instance_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}
ClapInstance ClapInstance::open(const std::string& path, const std::string& identifier)
{
    return ClapInstance(nylon_clap_instance_open(path.c_str(), identifier.c_str()));
}
bool ClapInstance::activate(
    double sampleRate, std::uint32_t minFrames, std::uint32_t maxFrames)
{
    return nylon_clap_instance_activate(m_handle, sampleRate, minFrames, maxFrames) != 0;
}
bool ClapInstance::processStereo(const float* inputLeft, const float* inputRight,
    float* outputLeft, float* outputRight, std::uint32_t frames)
{
    return nylon_clap_instance_process_stereo(
               m_handle, inputLeft, inputRight, outputLeft, outputRight, frames)
        != 0;
}
bool ClapInstance::processStereo(const float* inputLeft, const float* inputRight,
    float* outputLeft, float* outputRight, std::uint32_t frames,
    const ParameterEvent* parameterEvents, std::uint32_t parameterEventCount,
    const NoteEvent* noteEvents, std::uint32_t noteEventCount)
{
    static_assert(sizeof(ParameterEvent) == sizeof(NylonClapParameterEvent));
    static_assert(alignof(ParameterEvent) == alignof(NylonClapParameterEvent));
    static_assert(sizeof(NoteEvent) == sizeof(NylonClapNoteEvent));
    static_assert(alignof(NoteEvent) == alignof(NylonClapNoteEvent));
    return nylon_clap_instance_process_stereo_all_events(m_handle, inputLeft,
               inputRight, outputLeft, outputRight, frames,
               reinterpret_cast<const NylonClapParameterEvent*>(parameterEvents),
               parameterEventCount,
               reinterpret_cast<const NylonClapNoteEvent*>(noteEvents), noteEventCount)
        != 0;
}
std::uint32_t ClapInstance::inputNotePorts() const
{
    return nylon_clap_instance_input_note_ports(m_handle);
}
std::uint32_t ClapInstance::inputAudioPorts() const
{
    return nylon_clap_instance_input_audio_ports(m_handle);
}
bool ClapInstance::processStereo(const float* inputLeft, const float* inputRight,
    float* outputLeft, float* outputRight, std::uint32_t frames,
    const ParameterEvent* events, std::uint32_t eventCount)
{
    static_assert(sizeof(ParameterEvent) == sizeof(NylonClapParameterEvent));
    static_assert(alignof(ParameterEvent) == alignof(NylonClapParameterEvent));
    return nylon_clap_instance_process_stereo_events(m_handle, inputLeft, inputRight,
               outputLeft, outputRight, frames,
               reinterpret_cast<const NylonClapParameterEvent*>(events), eventCount)
        != 0;
}
std::vector<ClapInstance::ParameterInfo> ClapInstance::parameters() const
{
    std::vector<ParameterInfo> result;
    const auto count = nylon_clap_instance_parameter_count(m_handle);
    result.reserve(static_cast<std::size_t>(count));
    for (unsigned long long index = 0; index < count; ++index) {
        NylonClapParameterInfo info{};
        if (nylon_clap_instance_parameter_info(m_handle, index, &info) == 0) break;
        result.push_back({info.identifier, info.flags, info.name, info.module,
            info.minimum, info.maximum, info.default_value});
    }
    return result;
}
bool ClapInstance::parameterValue(std::uint32_t identifier, double& value) const
{
    return nylon_clap_instance_parameter_value(m_handle, identifier, &value) != 0;
}
bool ClapInstance::latency(std::uint32_t& frames) const
{
    return nylon_clap_instance_latency(m_handle, &frames) != 0;
}
bool ClapInstance::saveState(std::vector<std::uint8_t>& state) const
{
    void* saved = nylon_clap_instance_save_state(m_handle);
    if (saved == nullptr) return false;
    struct StateGuard {
        void* handle;
        ~StateGuard() { nylon_clap_state_free(handle); }
    } guard{saved};
    const auto size = nylon_clap_state_size(saved);
    const auto* data = nylon_clap_state_data(saved);
    if (size == 0)
        state.clear();
    else
        state.assign(data, data + size);
    return true;
}
bool ClapInstance::loadState(const std::vector<std::uint8_t>& state)
{
    return nylon_clap_instance_load_state(m_handle,
               state.empty() ? nullptr : state.data(),
               static_cast<unsigned long long>(state.size()))
        != 0;
}
bool ClapInstance::reset() { return nylon_clap_instance_reset(m_handle) != 0; }
std::uint32_t ClapInstance::takeRequests()
{
    return nylon_clap_instance_take_requests(m_handle);
}

ClapWorker::~ClapWorker() { nylon_clap_worker_free(m_handle); }
ClapWorker::ClapWorker(ClapWorker&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}
ClapWorker& ClapWorker::operator=(ClapWorker&& other) noexcept
{
    if (this != &other) {
        nylon_clap_worker_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}
ClapWorker ClapWorker::open(const std::string& executable, const std::string& path,
    const std::string& identifier, double sampleRate, std::uint32_t maxFrames)
{
    return ClapWorker(nylon_clap_worker_open(executable.c_str(), path.c_str(),
        identifier.c_str(), sampleRate, maxFrames));
}
bool ClapWorker::processStereo(const float* inputLeft, const float* inputRight,
    float* outputLeft, float* outputRight, std::uint32_t frames,
    const ParameterEvent* parameterEvents, std::uint32_t parameterEventCount,
    const NoteEvent* noteEvents, std::uint32_t noteEventCount)
{
    static_assert(sizeof(ParameterEvent) == sizeof(NylonClapParameterEvent));
    static_assert(alignof(ParameterEvent) == alignof(NylonClapParameterEvent));
    static_assert(sizeof(NoteEvent) == sizeof(NylonClapNoteEvent));
    static_assert(alignof(NoteEvent) == alignof(NylonClapNoteEvent));
    return nylon_clap_worker_process_stereo(m_handle, inputLeft, inputRight,
               outputLeft, outputRight, frames,
               reinterpret_cast<const NylonClapParameterEvent*>(parameterEvents),
               parameterEventCount,
               reinterpret_cast<const NylonClapNoteEvent*>(noteEvents), noteEventCount)
        != 0;
}
std::uint32_t ClapWorker::inputNotePorts() const
{
    return nylon_clap_worker_input_note_ports(m_handle);
}
std::uint32_t ClapWorker::inputAudioPorts() const
{
    return nylon_clap_worker_input_audio_ports(m_handle);
}
std::vector<ClapWorker::ParameterInfo> ClapWorker::parameters() const
{
    std::vector<ParameterInfo> result;
    const auto count = nylon_clap_worker_parameter_count(m_handle);
    result.reserve(static_cast<std::size_t>(count));
    for (unsigned long long index = 0; index < count; ++index) {
        NylonClapParameterInfo info{};
        if (nylon_clap_worker_parameter_info(m_handle, index, &info) == 0) break;
        result.push_back({info.identifier, info.flags, info.name, info.module,
            info.minimum, info.maximum, info.default_value});
    }
    return result;
}
bool ClapWorker::latency(std::uint32_t& frames) const
{
    return nylon_clap_worker_latency(m_handle, &frames) != 0;
}
bool ClapWorker::saveState(std::vector<std::uint8_t>& state)
{
    void* saved = nylon_clap_worker_save_state(m_handle);
    if (saved == nullptr) return false;
    struct StateGuard {
        void* handle;
        ~StateGuard() { nylon_clap_state_free(handle); }
    } guard{saved};
    const auto size = nylon_clap_state_size(saved);
    const auto* data = nylon_clap_state_data(saved);
    if (size == 0)
        state.clear();
    else
        state.assign(data, data + size);
    return true;
}
bool ClapWorker::loadState(const std::vector<std::uint8_t>& state)
{
    return nylon_clap_worker_load_state(m_handle,
               state.empty() ? nullptr : state.data(),
               static_cast<unsigned long long>(state.size()))
        != 0;
}

ClapBridge::~ClapBridge() { nylon_clap_bridge_free(m_handle); }
ClapBridge::ClapBridge(ClapBridge&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}
ClapBridge& ClapBridge::operator=(ClapBridge&& other) noexcept
{
    if (this != &other) {
        nylon_clap_bridge_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}
ClapBridge ClapBridge::open(const std::string& executable, const std::string& path,
    const std::string& identifier, double sampleRate, std::uint32_t frames,
    std::uint32_t queueDepth)
{
    return ClapBridge(nylon_clap_bridge_open(executable.c_str(), path.c_str(),
        identifier.c_str(), sampleRate, frames, queueDepth));
}
bool ClapBridge::processStereo(const float* inputLeft, const float* inputRight,
    float* outputLeft, float* outputRight, std::uint32_t frames,
    const ParameterEvent* parameterEvents, std::uint32_t parameterEventCount,
    const NoteEvent* noteEvents, std::uint32_t noteEventCount)
{
    static_assert(sizeof(ParameterEvent) == sizeof(NylonClapParameterEvent));
    static_assert(alignof(ParameterEvent) == alignof(NylonClapParameterEvent));
    static_assert(sizeof(NoteEvent) == sizeof(NylonClapNoteEvent));
    static_assert(alignof(NoteEvent) == alignof(NylonClapNoteEvent));
    return nylon_clap_bridge_process_stereo(m_handle, inputLeft, inputRight,
               outputLeft, outputRight, frames,
               reinterpret_cast<const NylonClapParameterEvent*>(parameterEvents),
               parameterEventCount,
               reinterpret_cast<const NylonClapNoteEvent*>(noteEvents), noteEventCount)
        != 0;
}
std::uint32_t ClapBridge::latency() const { return nylon_clap_bridge_latency(m_handle); }
bool ClapBridge::isRunning() const { return nylon_clap_bridge_is_running(m_handle) != 0; }
std::uint64_t ClapBridge::submittedBlocks() const
{
    return nylon_clap_bridge_submitted_blocks(m_handle);
}
std::uint64_t ClapBridge::completedBlocks() const
{
    return nylon_clap_bridge_completed_blocks(m_handle);
}
std::uint64_t ClapBridge::underruns() const
{
    return nylon_clap_bridge_underruns(m_handle);
}
std::uint64_t ClapBridge::queueDrops() const
{
    return nylon_clap_bridge_queue_drops(m_handle);
}
std::uint64_t ClapBridge::workerFailures() const
{
    return nylon_clap_bridge_worker_failures(m_handle);
}

CompiledRouting::CompiledRouting() = default;
CompiledRouting::CompiledRouting(void* handle)
    : m_handle(handle)
{
}
CompiledRouting::~CompiledRouting() { nylon_compiled_routing_free(m_handle); }
CompiledRouting::CompiledRouting(CompiledRouting&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}
CompiledRouting& CompiledRouting::operator=(CompiledRouting&& other) noexcept
{
    if (this != &other) {
        nylon_compiled_routing_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}
std::vector<std::uint32_t> CompiledRouting::order() const
{
    std::vector<std::uint32_t> result;
    const auto count = nylon_compiled_routing_node_count(m_handle);
    result.reserve(count);
    for (std::uint32_t index = 0; index < count; ++index) {
        const int node = nylon_compiled_routing_order_at(m_handle, index);
        if (node < 0) return {};
        result.push_back(static_cast<std::uint32_t>(node));
    }
    return result;
}
bool CompiledRouting::edgeDelay(std::uint32_t index, std::uint32_t& frames) const
{
    return nylon_compiled_routing_edge_delay(m_handle, index, &frames) != 0;
}
bool CompiledRouting::outputLatency(std::uint32_t node, std::uint32_t& frames) const
{
    return nylon_compiled_routing_output_latency(m_handle, node, &frames) != 0;
}

RoutingGraph::RoutingGraph(std::uint32_t nodeCount)
    : m_handle(nylon_routing_new(nodeCount))
{
}
RoutingGraph::~RoutingGraph() { nylon_routing_free(m_handle); }
RoutingGraph::RoutingGraph(RoutingGraph&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}
RoutingGraph& RoutingGraph::operator=(RoutingGraph&& other) noexcept
{
    if (this != &other) {
        nylon_routing_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}
bool RoutingGraph::setNodeLatency(std::uint32_t node, std::uint32_t frames)
{
    return nylon_routing_set_node_latency(m_handle, node, frames) != 0;
}
bool RoutingGraph::addEdge(std::uint32_t source, std::uint32_t destination, RoutingKind kind,
    float gain, std::uint32_t& index)
{
    return nylon_routing_add_edge(
               m_handle, source, destination, static_cast<int>(kind), gain, &index)
        != 0;
}
CompiledRouting RoutingGraph::compile() const
{
    return CompiledRouting(nylon_routing_compile(m_handle));
}

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
bool Project::save(const std::string& bundleDirectory)
{
    return nylon_project_save(m_handle, bundleDirectory.c_str()) != 0;
}
bool Project::open(const std::string& bundleDirectory)
{
    return nylon_project_open(m_handle, bundleDirectory.c_str()) != 0;
}
bool Project::isModified() const { return nylon_project_is_modified(m_handle) != 0; }
bool Project::autosave() const { return nylon_project_autosave(m_handle) != 0; }
bool Project::recoveryAvailable(const std::string& bundleDirectory)
{
    return nylon_project_recovery_available(bundleDirectory.c_str()) != 0;
}
bool Project::recover(const std::string& bundleDirectory)
{
    return nylon_project_recover(m_handle, bundleDirectory.c_str()) != 0;
}
bool Project::discardRecovery(const std::string& bundleDirectory)
{
    return nylon_project_discard_recovery(bundleDirectory.c_str()) != 0;
}
bool Project::bounceWave(const std::string& path, double startBeats, double endBeats,
    std::uint32_t sampleRate, BounceReport& report) const
{
    NylonBounceReport native{};
    if (nylon_render_bounce_wave(
            m_handle, path.c_str(), startBeats, endBeats, sampleRate, &native)
        == 0) {
        return false;
    }
    report = {native.frames, native.peak_left, native.peak_right};
    return true;
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
std::uint32_t Project::trackLatencyFrames(std::uint64_t index) const
{
    return nylon_track_latency_frames(m_handle, index);
}
bool Project::setTrackLatencyFrames(std::uint64_t index, std::uint32_t frames)
{
    return nylon_track_set_latency_frames(m_handle, index, frames) != 0;
}

namespace {
NylonTrackDevice nativeDevice(const TrackDevice& device)
{
    NylonTrackDevice result{};
    result.kind = static_cast<int>(device.kind);
    result.enabled = device.enabled ? 1 : 0;
    for (std::size_t index = 0; index < device.parameters.size(); ++index)
        result.parameters[index] = device.parameters[index];
    return result;
}

TrackDevice cppTrackDevice(const NylonTrackDevice& device)
{
    TrackDevice result;
    result.kind = static_cast<DeviceKind>(device.kind);
    result.enabled = device.enabled != 0;
    for (std::size_t index = 0; index < result.parameters.size(); ++index)
        result.parameters[index] = device.parameters[index];
    return result;
}

NylonInstrumentPatch nativeInstrument(const InstrumentPatch& patch)
{
    return {static_cast<int>(patch.shapeA), static_cast<int>(patch.shapeB),
        patch.oscillatorMix, patch.oscillatorBDetuneCents, patch.subLevel,
        patch.noiseLevel, patch.unisonVoices, patch.unisonDetuneCents,
        patch.attackSeconds, patch.decaySeconds, patch.sustain, patch.releaseSeconds,
        patch.cutoffHz, patch.resonance, patch.levelDb};
}

InstrumentPatch instrumentPatch(const NylonInstrumentPatch& patch)
{
    return {static_cast<OscillatorShape>(patch.shape_a),
        static_cast<OscillatorShape>(patch.shape_b), patch.oscillator_mix,
        patch.oscillator_b_detune_cents, patch.sub_level, patch.noise_level,
        patch.unison_voices, patch.unison_detune_cents, patch.attack_seconds,
        patch.decay_seconds, patch.sustain, patch.release_seconds, patch.cutoff_hz,
        patch.resonance, patch.level_db};
}
}

bool Project::trackInstrument(std::uint64_t track, InstrumentPatch& patch) const
{
    NylonInstrumentPatch native{};
    if (!nylon_track_instrument_get(m_handle, track, &native)) return false;
    patch = instrumentPatch(native);
    return true;
}

bool Project::setTrackInstrument(std::uint64_t track, const InstrumentPatch& patch)
{
    const auto native = nativeInstrument(patch);
    return nylon_track_instrument_set(m_handle, track, &native) != 0;
}

std::vector<TrackDevice> Project::trackDevices(std::uint64_t track) const
{
    std::vector<TrackDevice> result;
    const auto count = nylon_track_device_count(m_handle, track);
    result.reserve(static_cast<std::size_t>(count));
    for (std::uint64_t index = 0; index < count; ++index) {
        NylonTrackDevice device{};
        if (!nylon_track_device_get(m_handle, track, index, &device)) return {};
        result.push_back(cppTrackDevice(device));
    }
    return result;
}

std::uint64_t Project::trackDeviceCount(std::uint64_t track) const
{
    return nylon_track_device_count(m_handle, track);
}

int Project::trackDeviceType(std::uint64_t track, std::uint64_t index) const
{
    return nylon_track_device_type(m_handle, track, index);
}

bool Project::trackDevice(std::uint64_t track, std::uint64_t index, TrackDevice& device) const
{
    NylonTrackDevice native{};
    if (!nylon_track_device_get(m_handle, track, index, &native)) return false;
    device = cppTrackDevice(native);
    return true;
}

bool Project::trackPluginDevice(
    std::uint64_t track, std::uint64_t index, TrackPluginDevice& device) const
{
    const auto format = nylon_track_plugin_format(m_handle, track, index);
    if (format < 0) return false;
    const auto packageLength = nylon_track_plugin_package(m_handle, track, index, nullptr, 0);
    const auto identifierLength
        = nylon_track_plugin_identifier(m_handle, track, index, nullptr, 0);
    std::vector<char> package(static_cast<std::size_t>(packageLength) + 1);
    std::vector<char> identifier(static_cast<std::size_t>(identifierLength) + 1);
    nylon_track_plugin_package(m_handle, track, index, package.data(), package.size());
    nylon_track_plugin_identifier(m_handle, track, index, identifier.data(), identifier.size());
    const auto stateLength = nylon_track_plugin_state(m_handle, track, index, nullptr, 0);
    std::vector<std::uint8_t> state(static_cast<std::size_t>(stateLength));
    if (stateLength != 0)
        nylon_track_plugin_state(m_handle, track, index, state.data(), state.size());
    device = {static_cast<PluginFormat>(format),
        nylon_track_device_enabled(m_handle, track, index) == 1,
        package.data(), identifier.data(),
        nylon_track_plugin_latency(m_handle, track, index), std::move(state)};
    return true;
}

bool Project::addTrackDevice(std::uint64_t track, const TrackDevice& device)
{
    const auto native = nativeDevice(device);
    return nylon_track_device_add(m_handle, track, &native) != 0;
}

bool Project::addTrackPlugin(std::uint64_t track, const TrackPluginDevice& device)
{
    return nylon_track_plugin_add(m_handle, track, static_cast<int>(device.format),
               device.package.c_str(), device.identifier.c_str(), device.latencyFrames,
               device.state.empty() ? nullptr : device.state.data(), device.state.size(),
               device.enabled ? 1 : 0)
        != 0;
}

bool Project::setTrackDevice(
    std::uint64_t track, std::uint64_t index, const TrackDevice& device)
{
    const auto native = nativeDevice(device);
    return nylon_track_device_set(m_handle, track, index, &native) != 0;
}

bool Project::setTrackDeviceEnabled(std::uint64_t track, std::uint64_t index, bool enabled)
{
    return nylon_track_device_set_enabled(m_handle, track, index, enabled ? 1 : 0) != 0;
}

bool Project::setTrackPluginState(std::uint64_t track, std::uint64_t index,
    const std::vector<std::uint8_t>& state)
{
    return nylon_track_plugin_set_state(m_handle, track, index,
               state.empty() ? nullptr : state.data(), state.size())
        != 0;
}

bool Project::deleteTrackDevice(std::uint64_t track, std::uint64_t index)
{
    return nylon_track_device_delete(m_handle, track, index) != 0;
}

bool Project::moveTrackDevice(std::uint64_t track, std::uint64_t from, std::uint64_t to)
{
    return nylon_track_device_move(m_handle, track, from, to) != 0;
}

std::vector<AutomationPoint> Project::trackAutomation(
    std::uint64_t track, AutomationParameter parameter) const
{
    std::vector<AutomationPoint> result;
    const auto count = nylon_track_automation_count(m_handle, track, static_cast<int>(parameter));
    result.reserve(static_cast<std::size_t>(count));
    for (std::uint64_t index = 0; index < count; ++index) {
        NylonAutomationPoint point{};
        if (!nylon_track_automation_get(
                m_handle, track, static_cast<int>(parameter), index, &point))
            return {};
        result.push_back({point.beat, point.value, static_cast<AutomationCurve>(point.curve)});
    }
    return result;
}

bool Project::setTrackAutomation(std::uint64_t track, AutomationParameter parameter,
    const std::vector<AutomationPoint>& points)
{
    std::vector<NylonAutomationPoint> native;
    native.reserve(points.size());
    for (const auto& point : points)
        native.push_back({point.beat, point.value, static_cast<int>(point.curve)});
    return nylon_track_automation_set(m_handle, track, static_cast<int>(parameter),
               native.data(), static_cast<unsigned long long>(native.size()))
        != 0;
}

bool Project::clearTrackAutomation(std::uint64_t track, AutomationParameter parameter)
{
    return nylon_track_automation_clear(m_handle, track, static_cast<int>(parameter)) != 0;
}

std::vector<ProjectRoute> Project::routes() const
{
    std::vector<ProjectRoute> result;
    const auto count = nylon_project_route_count(m_handle);
    result.reserve(static_cast<std::size_t>(count));
    for (std::uint64_t index = 0; index < count; ++index) {
        result.push_back({nylon_project_route_source(m_handle, index),
            nylon_project_route_destination(m_handle, index),
            static_cast<RoutingKind>(nylon_project_route_kind(m_handle, index)),
            nylon_project_route_gain(m_handle, index)});
    }
    return result;
}
bool Project::addRoute(
    std::uint64_t source, std::uint64_t destination, RoutingKind kind, float gain)
{
    return nylon_project_route_add(
               m_handle, source, destination, static_cast<int>(kind), gain)
        != 0;
}
bool Project::deleteRoute(std::uint64_t index)
{
    return nylon_project_route_delete(m_handle, index) != 0;
}

std::uint64_t Project::sceneCount() const { return nylon_scene_count(m_handle); }
bool Project::createScene(const std::string& name) { return nylon_scene_create(m_handle, name.c_str()) != 0; }
bool Project::deleteScene(std::uint64_t scene) { return nylon_scene_delete(m_handle, scene) != 0; }
std::string Project::sceneName(std::uint64_t scene) const
{
    char small[128];
    const auto needed = nylon_scene_name(m_handle, scene, small, sizeof(small));
    if (needed < sizeof(small)) return std::string(small);
    std::string large(static_cast<std::size_t>(needed), '\0');
    nylon_scene_name(m_handle, scene, large.data(), needed + 1);
    return large;
}
bool Project::setSceneName(std::uint64_t scene, const std::string& name)
{
    return nylon_scene_set_name(m_handle, scene, name.c_str()) != 0;
}

bool Project::clipSlotOccupied(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_slot_state(m_handle, track, scene) != 0;
}
bool Project::createMidiClip(std::uint64_t track, std::uint64_t scene, double lengthBeats)
{
    return nylon_clip_create_midi(m_handle, track, scene, lengthBeats) != 0;
}
bool Project::importWave(std::uint64_t track, std::uint64_t scene,
    const std::string& sourcePath, double sourceTempo)
{
    return nylon_clip_import_wave(m_handle, sourcePath.c_str(), track, scene, sourceTempo) != 0;
}
bool Project::deleteClip(std::uint64_t track, std::uint64_t scene)
{
    return nylon_clip_delete(m_handle, track, scene) != 0;
}
std::string Project::clipName(std::uint64_t track, std::uint64_t scene) const
{
    char small[128];
    const auto needed = nylon_clip_name(m_handle, track, scene, small, sizeof(small));
    if (needed < sizeof(small)) return std::string(small);
    std::string large(static_cast<std::size_t>(needed), '\0');
    nylon_clip_name(m_handle, track, scene, large.data(), needed + 1);
    return large;
}
bool Project::setClipName(std::uint64_t track, std::uint64_t scene, const std::string& name)
{
    return nylon_clip_set_name(m_handle, track, scene, name.c_str()) != 0;
}
int Project::clipColorIndex(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_color_index(m_handle, track, scene);
}
bool Project::setClipColorIndex(std::uint64_t track, std::uint64_t scene, int color)
{
    return nylon_clip_set_color_index(m_handle, track, scene, color) != 0;
}
BeatRange Project::clipLoop(std::uint64_t track, std::uint64_t scene) const
{
    return {nylon_clip_loop_start(m_handle, track, scene), nylon_clip_loop_length(m_handle, track, scene)};
}
bool Project::setClipLoop(std::uint64_t track, std::uint64_t scene, BeatRange range)
{
    return nylon_clip_set_loop(m_handle, track, scene, range.startBeats, range.lengthBeats) != 0;
}
std::string Project::clipMediaPath(std::uint64_t track, std::uint64_t scene) const
{
    char small[128];
    const auto needed = nylon_clip_media_path(m_handle, track, scene, small, sizeof(small));
    if (needed < sizeof(small)) return std::string(small);
    std::string large(static_cast<std::size_t>(needed), '\0');
    nylon_clip_media_path(m_handle, track, scene, large.data(), needed + 1);
    return large;
}
double Project::clipAudioGainDb(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_audio_gain_db(m_handle, track, scene);
}
bool Project::setClipAudioGainDb(std::uint64_t track, std::uint64_t scene, double db)
{
    return nylon_clip_set_audio_gain_db(m_handle, track, scene, db) != 0;
}
bool Project::clipAudioReversed(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_audio_reverse(m_handle, track, scene) != 0;
}
bool Project::setClipAudioReversed(std::uint64_t track, std::uint64_t scene, bool enabled)
{
    return nylon_clip_set_audio_reverse(m_handle, track, scene, enabled ? 1 : 0) != 0;
}
bool Project::clipAudioWarped(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_audio_warp(m_handle, track, scene) != 0;
}
double Project::clipAudioSourceTempo(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_audio_source_tempo(m_handle, track, scene);
}
bool Project::setClipAudioWarp(
    std::uint64_t track, std::uint64_t scene, bool enabled, double sourceTempo)
{
    return nylon_clip_set_audio_warp(m_handle, track, scene, enabled ? 1 : 0, sourceTempo) != 0;
}
std::uint64_t Project::clipNoteCount(std::uint64_t track, std::uint64_t scene) const
{
    return nylon_clip_note_count(m_handle, track, scene);
}
bool Project::clipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index, MidiNote& note) const
{
    return nylon_clip_note_at(m_handle, track, scene, index, &note.pitch, &note.velocity,
               &note.startBeats, &note.lengthBeats)
        != 0;
}
bool Project::addClipNote(std::uint64_t track, std::uint64_t scene, MidiNote note)
{
    return nylon_clip_note_add(m_handle, track, scene, note.pitch, note.velocity, note.startBeats,
               note.lengthBeats)
        != 0;
}
bool Project::removeClipNote(std::uint64_t track, std::uint64_t scene, std::uint64_t index)
{
    return nylon_clip_note_remove(m_handle, track, scene, index) != 0;
}
bool Project::moveClipNote(
    std::uint64_t track, std::uint64_t scene, std::uint64_t index, MidiNote note)
{
    return nylon_clip_note_move(m_handle, track, scene, index, note.pitch, note.velocity,
               note.startBeats, note.lengthBeats)
        != 0;
}
bool Project::quantizeClipNotes(
    std::uint64_t track, std::uint64_t scene, double gridBeats, double strength)
{
    return nylon_clip_notes_quantize(m_handle, track, scene, gridBeats, strength) != 0;
}
bool Project::transposeClipNotes(std::uint64_t track, std::uint64_t scene, int semitones)
{
    return nylon_clip_notes_transpose(m_handle, track, scene, semitones) != 0;
}
bool Project::setClipNoteVelocity(
    std::uint64_t track, std::uint64_t scene, std::uint8_t velocity)
{
    return nylon_clip_notes_set_velocity(m_handle, track, scene, velocity) != 0;
}
bool Project::humanizeClipNotes(std::uint64_t track, std::uint64_t scene, double timingBeats,
    std::uint8_t velocityRange, std::uint64_t seed)
{
    return nylon_clip_notes_humanize(
               m_handle, track, scene, timingBeats, velocityRange, seed)
        != 0;
}

std::uint64_t Project::arrangementClipCount(std::uint64_t track) const
{
    return nylon_arrangement_clip_count(m_handle, track);
}
bool Project::addArrangementClipFromSlot(
    std::uint64_t track, std::uint64_t scene, BeatRange range)
{
    return nylon_arrangement_clip_add_from_slot(
               m_handle, track, scene, range.startBeats, range.lengthBeats)
        != 0;
}
bool Project::arrangementClipRange(
    std::uint64_t track, std::uint64_t index, BeatRange& range) const
{
    return nylon_arrangement_clip_range(
               m_handle, track, index, &range.startBeats, &range.lengthBeats)
        != 0;
}
std::string Project::arrangementClipName(std::uint64_t track, std::uint64_t index) const
{
    char small[128];
    const auto needed = nylon_arrangement_clip_name(m_handle, track, index, small, sizeof(small));
    if (needed < sizeof(small)) return std::string(small);
    std::string large(static_cast<std::size_t>(needed), '\0');
    nylon_arrangement_clip_name(m_handle, track, index, large.data(), needed + 1);
    return large;
}
int Project::arrangementClipColorIndex(std::uint64_t track, std::uint64_t index) const
{
    return nylon_arrangement_clip_color_index(m_handle, track, index);
}
bool Project::removeArrangementClip(std::uint64_t track, std::uint64_t index)
{
    return nylon_arrangement_clip_remove(m_handle, track, index) != 0;
}
bool Project::setArrangementClipRange(
    std::uint64_t track, std::uint64_t index, BeatRange range)
{
    return nylon_arrangement_clip_set_range(
               m_handle, track, index, range.startBeats, range.lengthBeats)
        != 0;
}

AudioEngine::AudioEngine()
    : m_handle(nylon_audio_new())
{
}

AudioEngine::~AudioEngine()
{
    nylon_audio_free(m_handle);
}

AudioEngine::AudioEngine(AudioEngine&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}

AudioEngine& AudioEngine::operator=(AudioEngine&& other) noexcept
{
    if (this != &other) {
        nylon_audio_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

std::vector<AudioDevice> AudioEngine::devices()
{
    const auto count = nylon_audio_device_list(nullptr, 0);
    std::vector<NylonAudioDevice> native(static_cast<std::size_t>(count));
    const auto written = nylon_audio_device_list(native.data(), count);
    std::vector<AudioDevice> result;
    result.reserve(static_cast<std::size_t>(written));
    for (std::uint64_t index = 0; index < written; ++index) {
        const auto& device = native[static_cast<std::size_t>(index)];
        AudioDevice converted;
        converted.id = device.id;
        converted.name = device.name;
        converted.channels = device.channels;
        converted.isDefault = device.is_default != 0;
        converted.sampleRates.assign(device.sample_rates,
            device.sample_rates + device.sample_rate_count);
        result.push_back(std::move(converted));
    }
    return result;
}

std::vector<AudioDevice> AudioEngine::inputDevices()
{
    const auto count = nylon_audio_input_device_list(nullptr, 0);
    std::vector<NylonAudioDevice> native(static_cast<std::size_t>(count));
    const auto written = nylon_audio_input_device_list(native.data(), count);
    std::vector<AudioDevice> result;
    result.reserve(static_cast<std::size_t>(written));
    for (std::uint64_t index = 0; index < written; ++index) {
        const auto& device = native[static_cast<std::size_t>(index)];
        AudioDevice converted;
        converted.id = device.id;
        converted.name = device.name;
        converted.channels = device.channels;
        converted.isDefault = device.is_default != 0;
        converted.sampleRates.assign(device.sample_rates,
            device.sample_rates + device.sample_rate_count);
        result.push_back(std::move(converted));
    }
    return result;
}

bool AudioEngine::defaultOutput(std::uint64_t& deviceId)
{
    unsigned long long nativeId = 0;
    if (nylon_audio_default_output(&nativeId) == 0) return false;
    deviceId = static_cast<std::uint64_t>(nativeId);
    return true;
}

bool AudioEngine::defaultInput(std::uint64_t& deviceId)
{
    unsigned long long nativeId = 0;
    if (nylon_audio_default_input(&nativeId) == 0) return false;
    deviceId = static_cast<std::uint64_t>(nativeId);
    return true;
}

bool AudioEngine::configurePluginHost(
    const std::string& worker, const std::vector<std::string>& roots)
{
    std::vector<const char*> nativeRoots;
    nativeRoots.reserve(roots.size());
    for (const auto& root : roots) nativeRoots.push_back(root.c_str());
    return nylon_audio_configure_plugin_host(m_handle, worker.c_str(),
               nativeRoots.empty() ? nullptr : nativeRoots.data(),
               static_cast<unsigned long long>(nativeRoots.size()))
        != 0;
}

bool AudioEngine::open(const Project& project, std::uint64_t deviceId,
    std::uint32_t sampleRate, std::uint32_t blockFrames)
{
    return nylon_audio_open(m_handle, project.raw(), deviceId, sampleRate, blockFrames) != 0;
}

bool AudioEngine::close() { return nylon_audio_close(m_handle) != 0; }
bool AudioEngine::isOpen() const { return nylon_audio_is_open(m_handle) != 0; }

bool AudioEngine::config(AudioConfig& config) const
{
    NylonAudioConfig native{};
    if (nylon_audio_config(m_handle, &native) == 0) return false;
    config = {native.device_id, native.sample_rate, native.block_frames, native.channels};
    return true;
}

bool AudioEngine::sync(const Project& project)
{
    return nylon_audio_sync(m_handle, project.raw()) != 0;
}

std::uint64_t AudioEngine::dropouts() const { return nylon_audio_dropouts(m_handle); }
std::uint64_t AudioEngine::framesRendered() const
{
    return nylon_audio_frames_rendered(m_handle);
}
bool AudioEngine::launchClip(const Project& project, std::uint64_t track, std::uint64_t scene,
    double quantizationBeats)
{
    return nylon_session_launch_clip(m_handle, project.raw(), track, scene, quantizationBeats)
        != 0;
}
bool AudioEngine::launchScene(
    const Project& project, std::uint64_t scene, double quantizationBeats)
{
    return nylon_session_launch_scene(m_handle, project.raw(), scene, quantizationBeats)
        != 0;
}
bool AudioEngine::stopSessionTrack(const Project& project, std::uint64_t track)
{
    return nylon_session_stop_track(m_handle, project.raw(), track) != 0;
}
std::int64_t AudioEngine::activeSessionScene(std::uint64_t track) const
{
    return nylon_session_active_scene(m_handle, track);
}
bool AudioEngine::play() { return nylon_transport_play(m_handle) != 0; }
bool AudioEngine::stop() { return nylon_transport_stop(m_handle) != 0; }
bool AudioEngine::locate(double beats) { return nylon_transport_locate(m_handle, beats) != 0; }
double AudioEngine::positionBeats() { return nylon_transport_position_beats(m_handle); }
bool AudioEngine::isPlaying() { return nylon_transport_is_playing(m_handle) != 0; }

namespace {
void copyLevels(const NylonLevels& native, Levels& levels)
{
    levels = {native.peak_left, native.peak_right, native.rms_left, native.rms_right,
        native.clipped != 0};
}
} // namespace

bool AudioEngine::trackLevels(std::uint64_t index, Levels& levels)
{
    NylonLevels native{};
    if (nylon_track_levels(m_handle, index, &native) == 0) return false;
    copyLevels(native, levels);
    return true;
}

bool AudioEngine::masterLevels(Levels& levels)
{
    NylonLevels native{};
    if (nylon_master_levels(m_handle, &native) == 0) return false;
    copyLevels(native, levels);
    return true;
}

Recording::~Recording()
{
    nylon_recording_free(m_handle);
}

Recording::Recording(Recording&& other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr))
{
}

Recording& Recording::operator=(Recording&& other) noexcept
{
    if (this != &other) {
        nylon_recording_free(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

bool Recording::open(const Project& project, std::uint64_t track, std::uint64_t scene,
    std::uint64_t deviceId, std::uint32_t sampleRate, std::uint32_t blockFrames)
{
    nylon_recording_free(m_handle);
    m_handle = nylon_recording_open(
        project.raw(), track, scene, deviceId, sampleRate, blockFrames);
    return m_handle != nullptr;
}

bool Recording::start()
{
    return nylon_recording_start(m_handle) != 0;
}

bool Recording::stop()
{
    return nylon_recording_stop(m_handle) != 0;
}

bool Recording::isRunning() const
{
    return nylon_recording_is_running(m_handle) != 0;
}

bool Recording::finish(Project& project, RecordingReport& report)
{
    NylonRecordingReport native{};
    if (nylon_recording_finish(m_handle, project.raw(), &native) == 0) return false;
    report = {native.frames, native.sample_rate, native.length_beats,
        native.lost_blocks, native.lost_frames};
    return true;
}

} // namespace nylon
