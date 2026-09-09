cmake_minimum_required(VERSION 3.21)
project(NylonControl VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

get_filename_component(NYLON_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/.." ABSOLUTE)
if(CMAKE_BINARY_DIR STREQUAL CMAKE_SOURCE_DIR)
    message(FATAL_ERROR "Configure into a separate directory, for example target/cli.")
endif()

# Strip the build host's source location from debug info and __FILE__ so
# binaries do not embed the path of the machine they were built on.
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
    add_compile_options(
        "-ffile-prefix-map=${NYLON_REPO_ROOT}=."
        "-fdebug-prefix-map=${NYLON_REPO_ROOT}=."
        "-fmacro-prefix-map=${NYLON_REPO_ROOT}=.")
elseif(MSVC)
    add_compile_options("/experimental:deterministic" "/pathmap:${NYLON_REPO_ROOT}=.")
endif()

# Core and Network only: this tool has no interface of its own.
find_package(Qt6 6.4 REQUIRED COMPONENTS Core Network)
add_subdirectory("${NYLON_REPO_ROOT}/bindings/cpp" bindings_cpp)

add_executable(nylon_control main.cpp control_server.cpp control_server.hpp device_codec.hpp)
target_link_libraries(nylon_control PRIVATE Qt6::Core Qt6::Network nyloncpp)
set_target_properties(nylon_control PROPERTIES OUTPUT_NAME "nylon-control")
if(WIN32)
    add_custom_command(TARGET nylon_control POST_BUILD
        COMMAND "${CMAKE_COMMAND}" -E copy_if_different
            "$<TARGET_FILE:nylon_core>" "$<TARGET_FILE_DIR:nylon_control>"
        VERBATIM)
endif()

include(GNUInstallDirs)
install(TARGETS nylon_control RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")

enable_testing()
find_package(Python3 COMPONENTS Interpreter REQUIRED)
add_library(nylon_clap_fixture MODULE tests/clap_fixture.c)
set_target_properties(nylon_clap_fixture PROPERTIES PREFIX "" SUFFIX ".clap")
add_library(nylon_clap_crash_fixture MODULE tests/clap_fixture.c)
target_compile_definitions(nylon_clap_crash_fixture PRIVATE NYLON_FIXTURE_CRASH=1)
set_target_properties(nylon_clap_crash_fixture PROPERTIES PREFIX "" SUFFIX ".clap")
add_library(nylon_clap_hang_fixture MODULE tests/clap_fixture.c)
target_compile_definitions(nylon_clap_hang_fixture PRIVATE NYLON_FIXTURE_HANG=1)
set_target_properties(nylon_clap_hang_fixture PROPERTIES PREFIX "" SUFFIX ".clap")
if(WIN32)
    set(NYLON_PLUGIN_PROBE_DEFAULT "${NYLON_REPO_ROOT}/target/release/nylon-plugin-probe.exe")
    set(NYLON_PLUGIN_WORKER_DEFAULT "${NYLON_REPO_ROOT}/target/release/nylon-plugin-worker.exe")
else()
    set(NYLON_PLUGIN_PROBE_DEFAULT "${NYLON_REPO_ROOT}/target/release/nylon-plugin-probe")
    set(NYLON_PLUGIN_WORKER_DEFAULT "${NYLON_REPO_ROOT}/target/release/nylon-plugin-worker")
endif()
set(NYLON_PLUGIN_PROBE "${NYLON_PLUGIN_PROBE_DEFAULT}"
    CACHE FILEPATH "Path to the Nylon plugin probe executable")
set(NYLON_PLUGIN_WORKER "${NYLON_PLUGIN_WORKER_DEFAULT}"
    CACHE FILEPATH "Path to the Nylon plugin worker executable")
install(PROGRAMS "${NYLON_PLUGIN_PROBE}" "${NYLON_PLUGIN_WORKER}"
    DESTINATION "${CMAKE_INSTALL_BINDIR}")
add_test(NAME cli_project COMMAND "${Python3_EXECUTABLE}"
    "${NYLON_REPO_ROOT}/scripts/test_cli_project.py" "$<TARGET_FILE:nylon_control>"
    "$<TARGET_FILE:nylon_clap_fixture>" "${NYLON_PLUGIN_PROBE}"
    "$<TARGET_FILE:nylon_clap_crash_fixture>" "${NYLON_PLUGIN_WORKER}")
add_test(NAME cli_server COMMAND "${Python3_EXECUTABLE}"
    "${NYLON_REPO_ROOT}/scripts/test_cli_server.py" "$<TARGET_FILE:nylon_control>")
add_test(NAME plugin_worker COMMAND "${Python3_EXECUTABLE}"
    "${NYLON_REPO_ROOT}/scripts/test_plugin_worker.py" "${NYLON_PLUGIN_WORKER}"
    "$<TARGET_FILE:nylon_clap_fixture>" "$<TARGET_FILE:nylon_clap_crash_fixture>")
add_executable(test_clap_worker tests/test_clap_worker.cpp)
target_link_libraries(test_clap_worker PRIVATE nyloncpp)
add_test(NAME cpp_clap_worker COMMAND test_clap_worker
    "${NYLON_PLUGIN_WORKER}" "$<TARGET_FILE:nylon_clap_fixture>"
    "$<TARGET_FILE:nylon_clap_hang_fixture>")
set_tests_properties(cli_server PROPERTIES TIMEOUT 20)
set_tests_properties(plugin_worker PROPERTIES TIMEOUT 20)
set_tests_properties(cpp_clap_worker PROPERTIES TIMEOUT 20)
