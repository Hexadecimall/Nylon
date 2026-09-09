#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define NYLON_EXPORT __declspec(dllexport)
#else
#define NYLON_EXPORT __attribute__((visibility("default")))
#endif

typedef struct ClapVersion {
    uint32_t major;
    uint32_t minor;
    uint32_t revision;
} ClapVersion;

typedef struct ClapDescriptor {
    ClapVersion version_abi;
    const char* id;
    const char* name;
    const char* vendor;
    const char* url;
    const char* manual_url;
    const char* support_url;
    const char* version;
    const char* description;
    const char* const* features;
} ClapDescriptor;

typedef struct ClapFactory ClapFactory;
typedef struct ClapPlugin ClapPlugin;
typedef struct ClapProcess ClapProcess;
typedef struct ClapAudioPortInfo ClapAudioPortInfo;
struct ClapFactory {
    uint32_t (*count)(const ClapFactory* factory);
    const ClapDescriptor* (*descriptor)(const ClapFactory* factory, uint32_t index);
    const ClapPlugin* (*create)(const ClapFactory* factory, const void* host, const char* id);
};

typedef struct ClapAudioBuffer {
    float** data32;
    double** data64;
    uint32_t channel_count;
    uint32_t latency;
    uint64_t constant_mask;
} ClapAudioBuffer;

struct ClapProcess {
    int64_t steady_time;
    uint32_t frames_count;
    const void* transport;
    const ClapAudioBuffer* audio_inputs;
    ClapAudioBuffer* audio_outputs;
    uint32_t audio_inputs_count;
    uint32_t audio_outputs_count;
    const void* in_events;
    const void* out_events;
};

struct ClapPlugin {
    const ClapDescriptor* descriptor;
    void* data;
    bool (*init)(const ClapPlugin* plugin);
    void (*destroy)(const ClapPlugin* plugin);
    bool (*activate)(const ClapPlugin* plugin, double sample_rate,
        uint32_t min_frames, uint32_t max_frames);
    void (*deactivate)(const ClapPlugin* plugin);
    bool (*start_processing)(const ClapPlugin* plugin);
    void (*stop_processing)(const ClapPlugin* plugin);
    void (*reset)(const ClapPlugin* plugin);
    int32_t (*process)(const ClapPlugin* plugin, const ClapProcess* process);
    const void* (*extension)(const ClapPlugin* plugin, const char* id);
    void (*on_main_thread)(const ClapPlugin* plugin);
};

struct ClapAudioPortInfo {
    uint32_t id;
    char name[256];
    uint32_t flags;
    uint32_t channel_count;
    const char* port_type;
    uint32_t in_place_pair;
};

typedef struct ClapAudioPorts {
    uint32_t (*count)(const ClapPlugin* plugin, bool input);
    bool (*get)(const ClapPlugin* plugin, uint32_t index, bool input,
        ClapAudioPortInfo* info);
} ClapAudioPorts;

typedef struct ClapEntry {
    ClapVersion version_abi;
    bool (*init)(const char* path);
    void (*deinit)(void);
    const void* (*factory)(const char* id);
} ClapEntry;

static const char* const fixture_features[] = {"audio-effect", "stereo", 0};
static const ClapDescriptor fixture_descriptor = {{1, 2, 2}, "app.nylon.fixture",
    "Fixture Effect", "Nylon Contributors", "", "", "", "1.0", "",
    fixture_features};

static uint32_t fixture_count(const ClapFactory* factory)
{
    (void)factory;
    return 1;
}

static const ClapDescriptor* fixture_get_descriptor(const ClapFactory* factory, uint32_t index)
{
    (void)factory;
    return index == 0 ? &fixture_descriptor : 0;
}

static bool fixture_plugin_active;
static bool fixture_plugin_processing;

static bool fixture_plugin_init(const ClapPlugin* plugin)
{
    fixture_plugin_active = false;
    fixture_plugin_processing = false;
    return plugin != 0;
}

static void fixture_plugin_destroy(const ClapPlugin* plugin)
{
    (void)plugin;
    fixture_plugin_active = false;
    fixture_plugin_processing = false;
}

static bool fixture_plugin_activate(const ClapPlugin* plugin, double sample_rate,
    uint32_t min_frames, uint32_t max_frames)
{
    (void)plugin;
    fixture_plugin_active = sample_rate > 0.0 && min_frames > 0 && max_frames >= min_frames;
    return fixture_plugin_active;
}

static void fixture_plugin_deactivate(const ClapPlugin* plugin)
{
    (void)plugin;
    fixture_plugin_active = false;
}

static bool fixture_plugin_start(const ClapPlugin* plugin)
{
    (void)plugin;
    fixture_plugin_processing = fixture_plugin_active;
    return fixture_plugin_processing;
}

static void fixture_plugin_stop(const ClapPlugin* plugin)
{
    (void)plugin;
    fixture_plugin_processing = false;
}

static void fixture_plugin_reset(const ClapPlugin* plugin) { (void)plugin; }

static uint32_t fixture_port_count(const ClapPlugin* plugin, bool input)
{
    (void)plugin;
    (void)input;
    return 1;
}

static bool fixture_port_get(const ClapPlugin* plugin, uint32_t index, bool input,
    ClapAudioPortInfo* info)
{
    (void)plugin;
    if (index != 0 || info == 0) return false;
    memset(info, 0, sizeof(*info));
    info->id = input ? 0 : 1;
    memcpy(info->name, input ? "Input" : "Output", input ? 6 : 7);
    info->flags = 1;
    info->channel_count = 2;
    info->port_type = "stereo";
    info->in_place_pair = input ? 1 : 0;
    return true;
}

static const ClapAudioPorts fixture_audio_ports = {fixture_port_count, fixture_port_get};

static const void* fixture_plugin_extension(const ClapPlugin* plugin, const char* id)
{
    (void)plugin;
    return id != 0 && strcmp(id, "clap.audio-ports") == 0 ? &fixture_audio_ports : 0;
}

static int32_t fixture_plugin_process(const ClapPlugin* plugin, const ClapProcess* process)
{
    (void)plugin;
    if (!fixture_plugin_processing || process == 0 || process->audio_inputs_count != 1
        || process->audio_outputs_count != 1 || process->audio_inputs == 0
        || process->audio_outputs == 0 || process->audio_inputs[0].channel_count != 2
        || process->audio_outputs[0].channel_count != 2
        || process->audio_inputs[0].data32 == 0 || process->audio_outputs[0].data32 == 0)
        return 0;
    for (uint32_t frame = 0; frame < process->frames_count; ++frame) {
        process->audio_outputs[0].data32[0][frame]
            = process->audio_inputs[0].data32[0][frame] * 0.5f;
        process->audio_outputs[0].data32[1][frame]
            = process->audio_inputs[0].data32[1][frame] * 0.5f;
    }
    return 1;
}

static void fixture_on_main_thread(const ClapPlugin* plugin) { (void)plugin; }

static const ClapPlugin fixture_plugin = {&fixture_descriptor, 0, fixture_plugin_init,
    fixture_plugin_destroy, fixture_plugin_activate, fixture_plugin_deactivate,
    fixture_plugin_start, fixture_plugin_stop, fixture_plugin_reset,
    fixture_plugin_process, fixture_plugin_extension, fixture_on_main_thread};

static const ClapPlugin* fixture_create(const ClapFactory* factory, const void* host, const char* id)
{
    (void)factory;
    return host != 0 && id != 0 && strcmp(id, fixture_descriptor.id) == 0
        ? &fixture_plugin
        : 0;
}

static const ClapFactory fixture_factory = {
    fixture_count, fixture_get_descriptor, fixture_create};

static bool fixture_init(const char* path)
{
#if defined(NYLON_FIXTURE_CRASH)
    abort();
#endif
    return path != 0 && path[0] != '\0';
}

static void fixture_deinit(void) {}

static const void* fixture_get_factory(const char* id)
{
    return id != 0 && strcmp(id, "clap.plugin-factory") == 0 ? &fixture_factory : 0;
}

NYLON_EXPORT const ClapEntry clap_entry = {
    {1, 2, 2}, fixture_init, fixture_deinit, fixture_get_factory};
