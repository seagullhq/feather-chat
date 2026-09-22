find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

set(_feather_win_manifest "")
if(WIN32)
    set(_feather_win_manifest app.manifest)
endif()

qt_add_executable(feather-chat
    src/main.cpp
    ${_feather_win_manifest}
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