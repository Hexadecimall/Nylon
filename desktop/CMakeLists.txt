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

find_package(Qt6 6.4 REQUIRED COMPONENTS Core Gui Widgets Test)

# The core library and its C++ binding.
add_subdirectory("${NYLON_REPO_ROOT}/bindings/cpp" bindings_cpp)

# Everything except main.cpp lives in a static library so the tests link the
# same objects as the application.
add_library(nylon_desktop STATIC
    src/LayoutMath.h
    src/widgets/ControlWidget.cpp
    src/widgets/ControlWidget.h
    src/widgets/Knob.cpp
    src/widgets/Knob.h
    src/widgets/Fader.cpp
    src/widgets/Fader.h
    src/widgets/LevelMeter.cpp
    src/widgets/LevelMeter.h
    src/widgets/FlatButton.cpp
    src/widgets/FlatButton.h
    src/widgets/ValueBox.cpp
    src/widgets/ValueBox.h
    src/BrowserPanel.cpp
    src/BrowserPanel.h
    src/DetailPanel.cpp
    src/DetailPanel.h
    src/MixerStrip.cpp
    src/MixerStrip.h
    src/MixerSection.cpp
    src/MixerSection.h
    src/StartScreen.cpp
    src/StartScreen.h
    src/PreferencesDialog.cpp
    src/PreferencesDialog.h
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
target_include_directories(nylon_desktop PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}/src")
target_link_libraries(nylon_desktop PUBLIC Qt6::Core Qt6::Gui Qt6::Widgets nyloncpp)
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
foreach(_test layout theme bridge widgets views)
    add_executable(test_${_test} tests/test_${_test}.cpp)
    target_link_libraries(test_${_test} PRIVATE nylon_desktop Qt6::Test)
    add_test(NAME ${_test} COMMAND test_${_test})
    # Widgets need a display; the offscreen platform works on every CI host.
    set_tests_properties(${_test} PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")
endforeach()
