cmake_minimum_required(VERSION 3.21)
project(NylonDesktop VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

# Keep build artifacts under the repository's target directory unless the
# caller picks another location.
get_filename_component(NYLON_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
if(CMAKE_BINARY_DIR STREQUAL CMAKE_SOURCE_DIR)
    message(FATAL_ERROR "Configure into a separate directory, for example target/desktop.")
endif()

# Strip the build host's source location from debug info and __FILE__ so
# binaries do not embed the path of the machine they were built on.
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
    add_compile_options(
        "-ffile-prefix-map=${NYLON_REPO_ROOT}=."
        "-fdebug-prefix-map=${NYLON_REPO_ROOT}=."
        "-fmacro-prefix-map=${NYLON_REPO_ROOT}=.")
elseif(MSVC)
    add_compile_options("/pathmap:${NYLON_REPO_ROOT}=.")
endif()

# Location of the Rust core static library. The default is the release
# build of the root crate; override with -DNYLON_CORE_LIBRARY=<path>.
if(WIN32 AND MSVC)
    set(_nylon_core_default "${NYLON_REPO_ROOT}/target/release/nylon.lib")
else()
    set(_nylon_core_default "${NYLON_REPO_ROOT}/target/release/libnylon.a")
endif()
set(NYLON_CORE_LIBRARY "${_nylon_core_default}" CACHE FILEPATH
    "Path to the Rust core static library (libnylon.a or nylon.lib)")

find_package(Qt6 6.4 REQUIRED COMPONENTS Core Gui Widgets Test)

add_library(nylon_core STATIC IMPORTED)
set_target_properties(nylon_core PROPERTIES IMPORTED_LOCATION "${NYLON_CORE_LIBRARY}")
# System libraries the Rust standard library links against.
if(APPLE)
    set_property(TARGET nylon_core PROPERTY INTERFACE_LINK_LIBRARIES
        "-framework CoreFoundation" "-framework Security" "-liconv")
elseif(WIN32)
    set_property(TARGET nylon_core PROPERTY INTERFACE_LINK_LIBRARIES
        ws2_32 userenv bcrypt ntdll advapi32 kernel32)
else()
    set_property(TARGET nylon_core PROPERTY INTERFACE_LINK_LIBRARIES
        pthread dl m gcc_s util rt)
endif()

# Everything except main.cpp lives in a static library so the tests link the
# same objects as the application.
add_library(nylon_desktop STATIC
    src/LayoutMath.h
    src/Theme.cpp
    src/Theme.h
    src/ThemeManager.cpp
    src/ThemeManager.h
    src/ProjectBridge.cpp
    src/ProjectBridge.h
    src/TransportBar.cpp
    src/TransportBar.h
    src/SessionView.cpp
    src/SessionView.h
    src/ArrangementView.cpp
    src/ArrangementView.h
    src/MainWindow.cpp
    src/MainWindow.h
    themes.qrc
)
target_include_directories(nylon_desktop PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}" "${CMAKE_CURRENT_SOURCE_DIR}/src")
target_link_libraries(nylon_desktop PUBLIC Qt6::Core Qt6::Gui Qt6::Widgets nylon_core)
target_compile_definitions(nylon_desktop PUBLIC NYLON_VERSION_STRING="${PROJECT_VERSION}")
if(MSVC)
    target_compile_options(nylon_desktop PUBLIC /W4 /permissive-)
else()
    target_compile_options(nylon_desktop PUBLIC -Wall -Wextra -Wpedantic -Wshadow -Wconversion)
endif()

add_executable(nylon MACOSX_BUNDLE WIN32 src/main.cpp)
target_link_libraries(nylon PRIVATE nylon_desktop)
set_target_properties(nylon PROPERTIES
    OUTPUT_NAME "Nylon"
    MACOSX_BUNDLE_BUNDLE_NAME "Nylon"
    MACOSX_BUNDLE_GUI_IDENTIFIER "app.nylon.daw"
    MACOSX_BUNDLE_BUNDLE_VERSION "${PROJECT_VERSION}"
    MACOSX_BUNDLE_SHORT_VERSION_STRING "${PROJECT_VERSION}"
    MACOSX_BUNDLE_COPYRIGHT "Copyright (c) Nylon Contributors")

enable_testing()
foreach(_test layout theme bridge views)
    add_executable(test_${_test} tests/test_${_test}.cpp)
    target_link_libraries(test_${_test} PRIVATE nylon_desktop Qt6::Test)
    add_test(NAME ${_test} COMMAND test_${_test})
    # Widgets need a display; the offscreen platform works on every CI host.
    set_tests_properties(${_test} PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")
endforeach()
