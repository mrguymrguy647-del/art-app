#pragma once

#include <string_view>

namespace art {

/// The engine's version as text, for example "0.1.0".
/// The number comes from project(... VERSION ...) in the top-level CMakeLists.txt.
std::string_view engineVersion();

} // namespace art
