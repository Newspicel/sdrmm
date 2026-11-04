/*
 * Burst: logs each block whose power rises above -20 dBFS.
 * The sdr-- plugin ABI in plain C, no libc. Build:
 *   zig cc -target wasm32-freestanding -O2 -nostdlib -Wl,--no-entry -Wl,--export-dynamic \
 *     -o burst.wasm burst.c
 */
#include <stddef.h>
#include <stdint.h>

#define EXPORT __attribute__((visibility("default")))
#define PAGE 65536
#define THRESHOLD 0.01f

enum { OP_LAYOUT = 1, OP_NORMALIZE = 2, OP_PRESENT = 3 };

static const char MANIFEST[] =
    "{\"abi\":1,"
    "\"descriptor\":{\"type_id\":\"burst\",\"name\":\"Burst\",\"summary\":\"Strong bursts\","
    "\"family\":\"utility\",\"bandwidth_hz\":12000.0,\"input_rate_hz\":48000.0,"
    "\"emits\":[\"burst\"]},"
    "\"kinds\":[{\"kind\":\"burst\",\"name\":\"Burst\",\"noun\":\"Bursts\",\"caps\":[\"log\"],"
    "\"hue\":\"amber\",\"lifetime\":{\"type\":\"instant\"}}]}";
static const char LAYOUT[] =
    "{\"ok\":{\"input_rate_hz\":48000.0,\"band\":[-6000.0,6000.0]}}";
static const char NORMALIZED[] = "{\"ok\":{}}";
static const char PRESENTED[] = "{\"ok\":{\"summary\":\"Burst\"}}";
static const char UNKNOWN[] = "{\"err\":\"unknown op\"}";
static const char EVENT[] = "[{\"kind\":\"burst\",\"data\":{}}]";
static const char LOCKED[] = "{\"health\":{\"lock\":true}}";
static const char UNLOCKED[] = "{\"health\":{\"lock\":false}}";
static const char NO_ERROR[] = "";

extern unsigned char __heap_base;
static uintptr_t heap_top = (uintptr_t)&__heap_base;

typedef struct {
    void *ptr;
    size_t cap;
} Region;

static Region inbox;
static Region input;
static uint32_t header[9];
static int loud;

static void *grab(size_t bytes) {
    uintptr_t at = (heap_top + 7) & ~(uintptr_t)7;
    uintptr_t end = at + bytes;
    size_t have = __builtin_wasm_memory_size(0) * PAGE;
    if (end > have && __builtin_wasm_memory_grow(0, (end - have + PAGE - 1) / PAGE) < 0)
        __builtin_trap();
    heap_top = end;
    return (void *)at;
}

static void *fit(Region *region, size_t bytes) {
    if (bytes > region->cap) {
        region->ptr = grab(bytes);
        region->cap = bytes;
    }
    return region->ptr;
}

static int64_t pack(const void *ptr, size_t len) {
    return ((int64_t)(uintptr_t)ptr << 32) | (int64_t)len;
}

EXPORT uint32_t sdrmm_abi(void) { return 1; }

EXPORT uint32_t sdrmm_alloc(uint32_t len) { return (uint32_t)(uintptr_t)fit(&inbox, len); }

EXPORT int64_t sdrmm_manifest(void) { return pack(MANIFEST, sizeof MANIFEST - 1); }

EXPORT int64_t sdrmm_call(int32_t op, uint32_t ptr, uint32_t len) {
    switch (op) {
    case OP_LAYOUT: return pack(LAYOUT, sizeof LAYOUT - 1);
    case OP_NORMALIZE: return pack(NORMALIZED, sizeof NORMALIZED - 1);
    case OP_PRESENT: return pack(PRESENTED, sizeof PRESENTED - 1);
    default: return pack(UNKNOWN, sizeof UNKNOWN - 1);
    }
}

EXPORT int32_t sdrmm_open(double rate, uint32_t ptr, uint32_t len) {
    loud = 0;
    return 0;
}

EXPORT int32_t sdrmm_apply(int32_t handle, uint32_t ptr, uint32_t len) { return 0; }

EXPORT uint32_t sdrmm_input(int32_t handle, uint32_t samples) {
    return (uint32_t)(uintptr_t)fit(&input, (size_t)samples * 2 * sizeof(float));
}

EXPORT uint32_t sdrmm_process(int32_t handle, uint32_t samples) {
    const float *iq = input.ptr;
    float power = 0.0f;
    for (uint32_t n = 0; n < 2 * samples; n++)
        power += iq[n] * iq[n];
    int now = samples > 0 && power / (float)samples > THRESHOLD;
    for (int word = 0; word < 9; word++)
        header[word] = 0;
    if (now && !loud) {
        header[3] = (uint32_t)(uintptr_t)EVENT;
        header[4] = sizeof EVENT - 1;
    }
    if (now != loud) {
        const char *status = now ? LOCKED : UNLOCKED;
        header[5] = (uint32_t)(uintptr_t)status;
        header[6] = now ? sizeof LOCKED - 1 : sizeof UNLOCKED - 1;
    }
    loud = now;
    return (uint32_t)(uintptr_t)header;
}

EXPORT void sdrmm_close(int32_t handle) {}

EXPORT int64_t sdrmm_error(void) { return pack(NO_ERROR, 0); }
