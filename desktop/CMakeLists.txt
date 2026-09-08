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

find_package(Qt6 6.4 REQUIRED COMPONENTS Core Gui Widgets Test Network)

# The core library and its C++ binding.
add_subdirectory("${NYLON_REPO_ROOT}/bindings/cpp" bindings_cpp)

# Everything except main.cpp lives in a static library so the tests link the
# same objects as the application.
add_library(nylon_desktop STATIC
    src/RemoteControl.cpp
    src/RemoteControl.h
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
    src/widgets/LcdDisplay.cpp
    src/widgets/LcdDisplay.h
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
    src/TitleBar.cpp
    src/TitleBar.h
    src/PreferencesDialog.cpp
    src/PreferencesDialog.h
    src/PianoRoll.cpp
    src/PianoRoll.h
    src/CommandPalette.cpp
    src/CommandPalette.h
    src/Shortcuts.cpp
    src/Shortcuts.h
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
target_link_libraries(nylon_desktop PUBLIC Qt6::Core Qt6::Gui Qt6::Widgets Qt6::Network nyloncpp)
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

# Portable install layout, the same on every platform:
#   <prefix>/Nylon/Bin         executables and qt.conf
#   <prefix>/Nylon/Frameworks  the core library and Qt runtime
#   <prefix>/Nylon/Resources   themes, icons, fonts
#   <prefix>/Nylon/Plugins     Qt platform/image plugins and nylon plugins
# On Windows the loader only searches the executable's directory, so the
# Qt runtime DLLs are installed beside the executables instead.
set(NYLON_INSTALL_ROOT "Nylon")
set(NYLON_BIN "${NYLON_INSTALL_ROOT}/Bin")
set(NYLON_FRAMEWORKS "${NYLON_INSTALL_ROOT}/Frameworks")
set(NYLON_RESOURCES "${NYLON_INSTALL_ROOT}/Resources")
set(NYLON_PLUGINS "${NYLON_INSTALL_ROOT}/Plugins")
if(APPLE)
    set_target_properties(nylon PROPERTIES INSTALL_RPATH "@executable_path/../Frameworks")
elseif(UNIX)
    # DT_RPATH (not RUNPATH) so the search path also covers libraries the
    # Qt plugins pull in after dlopen.
    set_target_properties(nylon PROPERTIES INSTALL_RPATH "$ORIGIN/../Frameworks")
    target_link_options(nylon PRIVATE "-Wl,--disable-new-dtags")
endif()

install(DIRECTORY "${NYLON_REPO_ROOT}/themes" DESTINATION "${NYLON_RESOURCES}")
install(DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/resources/icons" DESTINATION "${NYLON_RESOURCES}")
install(DIRECTORY DESTINATION "${NYLON_RESOURCES}/font")
install(DIRECTORY DESTINATION "${NYLON_PLUGINS}/nylon")
if(WIN32)
    file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/deploy/qt.conf" "[Paths]\nPrefix = ..\nPlugins = Plugins\nLibraries = Bin\n")
else()
    file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/deploy/qt.conf" "[Paths]\nPrefix = ..\nPlugins = Plugins\nLibraries = Frameworks\n")
endif()
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/deploy/qt.conf" DESTINATION "${NYLON_BIN}")

get_target_property(_qt_core_location Qt6::Core LOCATION)
get_filename_component(_qt_lib_dir "${_qt_core_location}" DIRECTORY)
if(APPLE)
    get_filename_component(_qt_lib_dir "${_qt_lib_dir}/../.." ABSOLUTE)
endif()
if(NOT DEFINED QT6_INSTALL_PLUGINS)
    set(QT6_INSTALL_PLUGINS "plugins")
endif()
if(IS_ABSOLUTE "${QT6_INSTALL_PLUGINS}")
    set(_qt_plugin_dir "${QT6_INSTALL_PLUGINS}")
else()
    set(_qt_plugin_dir "${QT6_INSTALL_PREFIX}/${QT6_INSTALL_PLUGINS}")
endif()
if(NOT DEFINED QT6_INSTALL_BINS)
    set(QT6_INSTALL_BINS "bin")
endif()
set(_qt_bin_dir "${QT6_INSTALL_PREFIX}/${QT6_INSTALL_BINS}")

if(APPLE)
    # macdeployqt rewrites install names relative to the executable; the
    # bundle it produces is then rearranged into the portable layout, which
    # keeps Bin/../Frameworks and Bin/../Plugins in the same places.
    find_program(NYLON_MACDEPLOYQT macdeployqt HINTS "${_qt_bin_dir}" "${QT6_INSTALL_PREFIX}/bin"
        "${QT6_INSTALL_PREFIX}/../qtbase/bin" "${QT6_INSTALL_PREFIX}/opt/qtbase/bin")
    if(NOT NYLON_MACDEPLOYQT)
        message(FATAL_ERROR "macdeployqt not found; set NYLON_MACDEPLOYQT")
    endif()
    install(CODE "
        set(_root \"\${CMAKE_INSTALL_PREFIX}/${NYLON_INSTALL_ROOT}\")
        set(_stage \"\${_root}/.deploy/Nylon.app\")
        file(REMOVE_RECURSE \"\${_root}/.deploy\")
        file(COPY \"$<TARGET_BUNDLE_DIR:nylon>\" DESTINATION \"\${_root}/.deploy\")
        execute_process(COMMAND \"${NYLON_MACDEPLOYQT}\" \"\${_stage}\" -always-overwrite
            RESULT_VARIABLE _deploy_result OUTPUT_QUIET ERROR_VARIABLE _deploy_err)
        if(NOT _deploy_result EQUAL 0)
            message(FATAL_ERROR \"macdeployqt failed: \${_deploy_err}\")
        endif()
        file(MAKE_DIRECTORY \"\${_root}/Bin\" \"\${_root}/Frameworks\" \"\${_root}/Plugins\")
        file(RENAME \"\${_stage}/Contents/MacOS/$<TARGET_FILE_NAME:nylon>\" \"\${_root}/Bin/$<TARGET_FILE_NAME:nylon>\")
        file(GLOB _frameworks \"\${_stage}/Contents/Frameworks/*\")
        foreach(_f IN LISTS _frameworks)
            get_filename_component(_n \"\${_f}\" NAME)
            file(REMOVE_RECURSE \"\${_root}/Frameworks/\${_n}\")
            file(RENAME \"\${_f}\" \"\${_root}/Frameworks/\${_n}\")
        endforeach()
        file(GLOB _plugin_dirs \"\${_stage}/Contents/PlugIns/*\")
        foreach(_d IN LISTS _plugin_dirs)
            get_filename_component(_n \"\${_d}\" NAME)
            file(REMOVE_RECURSE \"\${_root}/Plugins/\${_n}\")
            file(RENAME \"\${_d}\" \"\${_root}/Plugins/\${_n}\")
        endforeach()
        file(REMOVE_RECURSE \"\${_root}/.deploy\")
        # Rewriting install names invalidates the code signatures, and the
        # loader refuses unsigned code on Apple silicon; sign everything
        # ad hoc. A distribution signs with a real identity afterwards.
        file(GLOB _to_sign \"\${_root}/Frameworks/*.framework\" \"\${_root}/Frameworks/*.dylib\")
        file(GLOB_RECURSE _plugin_libs \"\${_root}/Plugins/*.dylib\")
        list(APPEND _to_sign \${_plugin_libs} \"\${_root}/Bin/$<TARGET_FILE_NAME:nylon>\")
        foreach(_item IN LISTS _to_sign)
            execute_process(COMMAND codesign --force --sign - --timestamp=none \"\${_item}\"
                RESULT_VARIABLE _sign_result ERROR_VARIABLE _sign_err OUTPUT_QUIET)
            if(NOT _sign_result EQUAL 0)
                message(FATAL_ERROR \"codesign failed for \${_item}: \${_sign_err}\")
            endif()
        endforeach()
    ")
elseif(WIN32)
    find_program(NYLON_WINDEPLOYQT windeployqt HINTS "${_qt_bin_dir}" "${QT6_INSTALL_PREFIX}/bin")
    # For a cross-compiled kit, windeployqt from the host kit needs the
    # target kit's qtpaths to find the right libraries.
    set(NYLON_WINDEPLOYQT_QTPATHS "" CACHE FILEPATH "qtpaths of the target Qt kit, when deploying with a host windeployqt")
    if(NOT NYLON_WINDEPLOYQT)
        message(FATAL_ERROR "windeployqt not found; set NYLON_WINDEPLOYQT")
    endif()
    install(PROGRAMS "$<TARGET_FILE:nylon>" DESTINATION "${NYLON_BIN}")
    install(CODE "
        set(_root \"\${CMAKE_INSTALL_PREFIX}/${NYLON_INSTALL_ROOT}\")
        set(_qtpaths_arg)
        if(NOT \"${NYLON_WINDEPLOYQT_QTPATHS}\" STREQUAL \"\")
            set(_qtpaths_arg --qtpaths \"${NYLON_WINDEPLOYQT_QTPATHS}\")
        endif()
        execute_process(COMMAND \"${NYLON_WINDEPLOYQT}\" \${_qtpaths_arg} --release --no-translations
            --no-system-d3d-compiler --no-opengl-sw --no-compiler-runtime
            --dir \"\${_root}/Bin\" --plugindir \"\${_root}/Plugins\"
            \"\${_root}/Bin/$<TARGET_FILE_NAME:nylon>\"
            RESULT_VARIABLE _deploy_result OUTPUT_QUIET ERROR_VARIABLE _deploy_err)
        if(NOT _deploy_result EQUAL 0)
            message(FATAL_ERROR \"windeployqt failed: \${_deploy_err}\")
        endif()
    ")
else()
    install(PROGRAMS "$<TARGET_FILE:nylon>" DESTINATION "${NYLON_BIN}")
    # Plugin directories may be symlink trees, so each file is resolved to
    # its real path before copying.
    install(CODE "
        foreach(_plugin_dir platforms imageformats iconengines styles xcbglintegrations wayland-shell-integration wayland-decoration-client platformthemes)
            file(REAL_PATH \"${_qt_plugin_dir}/\${_plugin_dir}\" _real_dir)
            if(NOT IS_DIRECTORY \"\${_real_dir}\")
                continue()
            endif()
            file(GLOB _entries \"\${_real_dir}/*.so\")
            foreach(_entry IN LISTS _entries)
                get_filename_component(_name \"\${_entry}\" NAME)
                if(_name MATCHES \"minimal|offscreen|vnc|linuxfb|eglfs|vkkhr\")
                    continue()
                endif()
                file(REAL_PATH \"\${_entry}\" _real)
                file(COPY \"\${_real}\" DESTINATION \"\${CMAKE_INSTALL_PREFIX}/${NYLON_PLUGINS}/\${_plugin_dir}\")
            endforeach()
        endforeach()
        set(_exe \"\${CMAKE_INSTALL_PREFIX}/${NYLON_BIN}/$<TARGET_FILE_NAME:nylon>\")
        file(GLOB_RECURSE _plugins LIST_DIRECTORIES false \"\${CMAKE_INSTALL_PREFIX}/${NYLON_PLUGINS}/*.so\")
        # Qt and the libraries only Qt needs travel with the application;
        # everything else comes from the system.
        file(GET_RUNTIME_DEPENDENCIES
            EXECUTABLES \"\${_exe}\"
            MODULES \${_plugins}
            RESOLVED_DEPENDENCIES_VAR _resolved
            UNRESOLVED_DEPENDENCIES_VAR _unresolved
            CONFLICTING_DEPENDENCIES_PREFIX _conflicts
            DIRECTORIES \"${_qt_lib_dir}\"
            POST_INCLUDE_REGEXES \"libQt6\" \"libicu\" \"libdouble-conversion\" \"libmd4c\" \"libb2\" \"libpcre2-16\" \"libxkbcommon\"
            POST_EXCLUDE_REGEXES \".*\")
        foreach(_dep IN LISTS _resolved)
            file(COPY \"\${_dep}\" DESTINATION \"\${CMAKE_INSTALL_PREFIX}/${NYLON_FRAMEWORKS}\" FOLLOW_SYMLINK_CHAIN)
        endforeach()
        if(_unresolved)
            message(WARNING \"Unresolved runtime dependencies: \${_unresolved}\")
        endif()
    ")
endif()

enable_testing()
foreach(_test layout theme bridge widgets views control commands pianoroll)
    add_executable(test_${_test} tests/test_${_test}.cpp)
    target_link_libraries(test_${_test} PRIVATE nylon_desktop Qt6::Test)
    if(WIN32)
        # Qt Test output does not reach the console on Windows runners, so
        # each suite also writes a plain-text log the workflow prints.
        add_test(NAME ${_test} COMMAND test_${_test} -o - -o "${CMAKE_CURRENT_BINARY_DIR}/Testing/qt-${_test}.txt,txt")
    else()
        add_test(NAME ${_test} COMMAND test_${_test})
    endif()
    # Widgets need a display; the offscreen platform works on every CI host.
    set_tests_properties(${_test} PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")
endforeach()

add_executable(nylon_control ../cli/main.cpp)
target_link_libraries(nylon_control PRIVATE Qt6::Core Qt6::Network nyloncpp)
set_target_properties(nylon_control PROPERTIES OUTPUT_NAME "nylon-control")
install(TARGETS nylon_control RUNTIME DESTINATION "${NYLON_BIN}")
find_package(Python3 COMPONENTS Interpreter REQUIRED)
add_test(NAME control_process COMMAND "${Python3_EXECUTABLE}"
    "${NYLON_REPO_ROOT}/scripts/test_control_process.py"
    "$<TARGET_FILE:nylon>" "$<TARGET_FILE:nylon_control>")
add_test(NAME cli_project COMMAND "${Python3_EXECUTABLE}"
    "${NYLON_REPO_ROOT}/scripts/test_cli_project.py" "$<TARGET_FILE:nylon_control>")
