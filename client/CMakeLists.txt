find_package(Qt6 REQUIRED COMPONENTS Widgets Network Multimedia)

qt_standard_project_setup()

set(assets "${CMAKE_CURRENT_SOURCE_DIR}/assets")

# Globs instead of lists, so new files need no edit here. CONFIGURE_DEPENDS
# re-runs the glob at build time; the headers are included for AUTOMOC.
file(GLOB_RECURSE FEATHER_SOURCES CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/src/*.hpp"
)

# Qt generates a manifest whose <trustInfo> is in the asm.v2 namespace (must be
# asm.v3), so Windows refuses to load the exe. A .manifest source makes Qt skip
# its own and CMake embeds ours unchanged.
set(FEATHER_MANIFEST "")
if(WIN32)
    set(FEATHER_MANIFEST app.manifest)
endif()

qt_add_executable(feather-chat ${FEATHER_SOURCES} ${FEATHER_MANIFEST})

# qt_add_resources FILES takes no wildcard, so glob the icons. Only *.svg
# matches, which keeps the LICENSE beside them out.
file(GLOB FEATHER_ICONS CONFIGURE_DEPENDS "${assets}/icons/*.svg")
qt_add_resources(feather-chat "icons"
    PREFIX "/icons"
    BASE "${assets}/icons"
    FILES ${FEATHER_ICONS}
)

# The window icon is the product mark, not a UI icon, so it sits beside icons/
# and is exposed as :/feather.svg.
qt_add_resources(feather-chat "appicon"
    PREFIX "/"
    BASE "${assets}"
    FILES "${assets}/feather.svg"
)

# Release and debug share one output directory with Ninja; without the per-config
# property the debug build would overwrite the release binary.
set_target_properties(feather-chat PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/dist/client"
    RUNTIME_OUTPUT_DIRECTORY_DEBUG "${CMAKE_SOURCE_DIR}/dist/client-debug"
)

# windeployqt cannot run in static vcpkg builds (no qtpaths).
if(WIN32)
    set_target_properties(feather-chat PROPERTIES QT_NO_DEPLOY ON)
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets Qt6::Network Qt6::Multimedia)

# clangd finds compile_commands.json by walking up from each source, but the
# database lives in dist/, a sibling of src/. Copy it to the repo root so it is
# found on whichever preset built it. An always-run target, not a POST_BUILD on
# the exe, because the copy must survive builds that relink nothing.
if(CMAKE_EXPORT_COMPILE_COMMANDS)
    add_custom_target(feather-compile-commands ALL
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${CMAKE_BINARY_DIR}/compile_commands.json"
            "${CMAKE_SOURCE_DIR}/compile_commands.json"
        VERBATIM
    )
endif()
