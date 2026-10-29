#include "artengine/version.h"

namespace art {

std::string_view engineVersion()
{
    // ARTENGINE_VERSION is defined by engine/CMakeLists.txt.
    return ARTENGINE_VERSION;
}

} // namespace art
