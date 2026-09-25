find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

set(_feather_win_manifest "")
if(WIN32)
    set(_feather_win_manifest app.manifest)
endif()

# CONFIGURE_DEPENDS makes the generator re-run this glob at build time, so
# new/removed files under src/ are picked up without manual re-configure.
# Headers are included for IDE / AUTOMOC purposes.
file(GLOB_RECURSE FEATHER_CHAT_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/*.hpp"
)

qt_add_executable(feather-chat
    ${FEATHER_CHAT_SOURCES}
    ${_feather_win_manifest}
)

qt_add_resources(feather-chat "icons"
    PREFIX "/icons"
    BASE "${CMAKE_CURRENT_SOURCE_DIR}/assets/icons"
    FILES
        assets/icons/server.svg
        assets/icons/microphone.svg
        assets/icons/microphone-slash.svg
        assets/icons/ear.svg
        assets/icons/ear-slash.svg
        assets/icons/home.svg
        assets/icons/plus.svg
)

set_target_properties(feather-chat PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/dist/client"
)

# QT_NO_DEPLOY skips windeployqt (qtpaths is missing in static vcpkg builds).
#
# Windows manifest: Qt 6 auto-generates one from app.exe.manifest.in, but that
# template puts <trustInfo> in the asm.v2 namespace (must be asm.v3), so the
# linked exe gets a manifest Windows refuses to load (SxS configuration error).
# Qt skips its own manifest generation when the target already has a .manifest
# source (QtPublicWindowsHelpers.cmake), and CMake then passes ours to
# lld-link via /MANIFESTINPUT, embedding it unchanged. No Windows SDK / mt.exe
# needed, and llvm-mt cannot write into an exe resource.
if(WIN32)
    set_target_properties(feather-chat PROPERTIES QT_NO_DEPLOY ON)
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
