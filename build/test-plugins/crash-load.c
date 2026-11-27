/* A VST3 bundle that scans fine (one instrument, "Crash On Load") but crashes when the host
 * creates an instance: onezan dies while loading it. For checking by hand that the next start
 * does not pick it, and that clicking it tries it again (DESIGN.md, "Plugins that crash
 * onezan"). Only what a scan reads is implemented. */
#include <stdint.h>
#include <string.h>

#ifdef _WIN32
#define EXPORT __declspec(dllexport)
#else
#define EXPORT __attribute__((visibility("default")))
#endif

typedef struct {
    char cid[16];
    int32_t cardinality;
    char category[32];
    char name[64];
} PClassInfo;

typedef struct {
    char cid[16];
    int32_t cardinality;
    char category[32];
    char name[64];
    uint32_t class_flags;
    char sub_categories[128];
    char vendor[64];
    char version[64];
    char sdk_version[64];
} PClassInfo2;

typedef struct {
    char vendor[64];
    char url[256];
    char email[128];
    int32_t flags;
} PFactoryInfo;

struct Factory;
typedef struct {
    int32_t (*query_interface)(struct Factory *, const char *, void **);
    uint32_t (*add_ref)(struct Factory *);
    uint32_t (*release)(struct Factory *);
    int32_t (*get_factory_info)(struct Factory *, PFactoryInfo *);
    int32_t (*count_classes)(struct Factory *);
    int32_t (*get_class_info)(struct Factory *, int32_t, PClassInfo *);
    int32_t (*create_instance)(struct Factory *, const char *, const char *, void **);
    int32_t (*get_class_info2)(struct Factory *, int32_t, PClassInfo2 *);
} Vtable;
struct Factory {
    const Vtable *vtable;
};

static const char CID[16] = "onezan-crashload";

/* Answers to every interface: the host only asks this factory for IPluginFactory2. */
static int32_t query_interface(struct Factory *f, const char *iid, void **out) {
    (void)iid;
    *out = f;
    return 0;
}
static uint32_t add_ref(struct Factory *f) { (void)f; return 1; }
static uint32_t release(struct Factory *f) { (void)f; return 1; }
static int32_t get_factory_info(struct Factory *f, PFactoryInfo *info) {
    (void)f;
    memset(info, 0, sizeof *info);
    strcpy(info->vendor, "onezan test");
    return 0;
}
static int32_t count_classes(struct Factory *f) { (void)f; return 1; }
static int32_t get_class_info(struct Factory *f, int32_t i, PClassInfo *c) {
    (void)f; (void)i;
    memset(c, 0, sizeof *c);
    memcpy(c->cid, CID, 16);
    c->cardinality = 0x7FFFFFFF;
    strcpy(c->category, "Audio Module Class");
    strcpy(c->name, "Crash On Load");
    return 0;
}
static int32_t get_class_info2(struct Factory *f, int32_t i, PClassInfo2 *c) {
    (void)f; (void)i;
    memset(c, 0, sizeof *c);
    memcpy(c->cid, CID, 16);
    c->cardinality = 0x7FFFFFFF;
    strcpy(c->category, "Audio Module Class");
    strcpy(c->name, "Crash On Load");
    strcpy(c->sub_categories, "Instrument|Synth");
    strcpy(c->vendor, "onezan test");
    return 0;
}
static int32_t create_instance(struct Factory *f, const char *cid, const char *iid, void **out) {
    (void)f; (void)cid; (void)iid; (void)out;
    *(volatile int *)0 = 0;
    return 1;
}

static const Vtable VTABLE = {
    query_interface, add_ref, release, get_factory_info,
    count_classes, get_class_info, create_instance, get_class_info2,
};
static struct Factory FACTORY = {&VTABLE};

EXPORT void *GetPluginFactory(void) { return &FACTORY; }
