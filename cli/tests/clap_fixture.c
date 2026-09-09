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
struct ClapFactory {
    uint32_t (*count)(const ClapFactory* factory);
    const ClapDescriptor* (*descriptor)(const ClapFactory* factory, uint32_t index);
    const void* (*create)(const ClapFactory* factory, const void* host, const char* id);
};

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

static const void* fixture_create(const ClapFactory* factory, const void* host, const char* id)
{
    (void)factory;
    (void)host;
    (void)id;
    return 0;
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
