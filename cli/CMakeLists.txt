cmake_minimum_required(VERSION 3.21)
project(NylonControl VERSION 0.1.0 LANGUAGES CXX)

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
    add_compile_options("/pathmap:${NYLON_REPO_ROOT}=.")
endif()

# Core and Network only: this tool has no interface of its own.
find_package(Qt6 6.4 REQUIRED COMPONENTS Core Network)
add_subdirectory("${NYLON_REPO_ROOT}/bindings/cpp" bindings_cpp)

add_executable(nylon_control main.cpp)
target_link_libraries(nylon_control PRIVATE Qt6::Core Qt6::Network nyloncpp)
set_target_properties(nylon_control PROPERTIES OUTPUT_NAME "nylon-control")

include(GNUInstallDirs)
install(TARGETS nylon_control RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")

enable_testing()
find_package(Python3 COMPONENTS Interpreter REQUIRED)
add_test(NAME cli_project COMMAND "${Python3_EXECUTABLE}"
    "${NYLON_REPO_ROOT}/scripts/test_cli_project.py" "$<TARGET_FILE:nylon_control>")
