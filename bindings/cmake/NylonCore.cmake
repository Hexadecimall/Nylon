# Defines the imported nylon_core target for the Rust core library.
# NYLON_CORE_LIBRARY selects the file; the default is the release static
# library built by cargo at the repository root.
if(TARGET nylon_core)
    return()
endif()

get_filename_component(_nylon_root "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
if(WIN32 AND MSVC)
    set(_nylon_core_default "${_nylon_root}/target/release/nylon.lib")
else()
    set(_nylon_core_default "${_nylon_root}/target/release/libnylon.a")
endif()
set(NYLON_CORE_LIBRARY "${_nylon_core_default}" CACHE FILEPATH
    "Path to the Rust core library (static or shared)")

get_filename_component(_nylon_core_ext "${NYLON_CORE_LIBRARY}" LAST_EXT)
if(_nylon_core_ext MATCHES "\\.(dylib|so|dll)$")
    add_library(nylon_core SHARED IMPORTED GLOBAL)
    if(WIN32)
        string(REGEX REPLACE "\\.dll$" ".dll.lib" _nylon_implib "${NYLON_CORE_LIBRARY}")
        set_target_properties(nylon_core PROPERTIES IMPORTED_IMPLIB "${_nylon_implib}")
    endif()
else()
    add_library(nylon_core STATIC IMPORTED GLOBAL)
endif()
set_target_properties(nylon_core PROPERTIES IMPORTED_LOCATION "${NYLON_CORE_LIBRARY}")
target_include_directories(nylon_core INTERFACE "${_nylon_root}/bindings/c")

# System libraries the Rust standard library links against when the core
# is linked statically.
if(NOT _nylon_core_ext MATCHES "\\.(dylib|so|dll)$")
    if(APPLE)
        # CoreFoundation, Security and iconv are what the Rust standard
        # library needs; AudioToolbox and CoreAudio are what the audio
        # backend calls into. A static core carries no record of them, so
        # anything linking it has to name them.
        set_property(TARGET nylon_core PROPERTY INTERFACE_LINK_LIBRARIES
            "-framework CoreFoundation" "-framework Security" "-liconv"
            "-framework AudioToolbox" "-framework CoreAudio")
    elseif(WIN32)
        set_property(TARGET nylon_core PROPERTY INTERFACE_LINK_LIBRARIES
            ws2_32 userenv bcrypt ntdll advapi32 kernel32)
    else()
        set_property(TARGET nylon_core PROPERTY INTERFACE_LINK_LIBRARIES
            pthread dl m gcc_s util rt)
    endif()
endif()
