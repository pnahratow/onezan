/* A VST3 bundle that crashes as soon as the host asks for its factory: onezan dies while
 * scanning it. For checking by hand that the next start skips it (DESIGN.md, "Plugins that
 * crash onezan"). */
#ifdef _WIN32
#define EXPORT __declspec(dllexport)
#else
#define EXPORT __attribute__((visibility("default")))
#endif

EXPORT void *GetPluginFactory(void) {
    *(volatile int *)0 = 0;
    return 0;
}
