# C++ binding library. Include from another CMake project with
# add_subdirectory and link the nyloncpp target; it carries the core
# library and the C header directory with it.
cmake_minimum_required(VERSION 3.21)
project(NylonCpp VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

get_filename_component(NYLON_BINDINGS_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
include("${NYLON_BINDINGS_ROOT}/cmake/NylonCore.cmake")

add_library(nyloncpp STATIC nylon.cpp nylon.hpp)
target_include_directories(nyloncpp PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}" "${NYLON_BINDINGS_ROOT}/c")
target_link_libraries(nyloncpp PUBLIC nylon_core)
set_target_properties(nyloncpp PROPERTIES OUTPUT_NAME "nyloncpp" PUBLIC_HEADER "nylon.hpp")
if(MSVC)
    target_compile_options(nyloncpp PRIVATE /W4 /permissive-)
else()
    target_compile_options(nyloncpp PRIVATE -Wall -Wextra -Wpedantic -Wshadow -Wconversion)
endif()
