// Thin extern "C" wrapper for the Qt Quick Test entry points. They are
// plain exported C++ functions, but their symbols are mangled; compiling
// this translation unit with the headers available keeps the name handling
// on the C++ side and exposes stable C symbols to the Rust runner.
//
// Compiled unconditionally by build.rs (see the QuickTest block there).
#include <QtQuickTest/quicktest.h>

extern "C" int rhesis_quick_test_main(
    int argc,
    char **argv,
    const char *name,
    const char *sourceDir)
{
    return quick_test_main(argc, argv, name, sourceDir);
}
