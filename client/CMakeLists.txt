find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

qt_add_executable(feather-chat
    src/main.cpp
)

# Qt/vcpkg generates a broken <requestedExecutionLevel /> tag (missing the
# required "level" attribute) causing a Windows SxS launch failure.
# Fix: embed our correct app.manifest via app.rc, then pass /MANIFEST:NO to
# prevent the linker from generating a second conflicting manifest on top.
# QT_NO_WIN32_MANIFEST suppresses Qt's own .manifest.res generation.
# QT_NO_DEPLOY skips windeployqt (qtpaths missing in static vcpkg builds).
if(WIN32)
    target_sources(feather-chat PRIVATE app.rc)
    target_link_options(feather-chat PRIVATE /MANIFEST:NO)
    set_target_properties(feather-chat PROPERTIES
        QT_NO_DEPLOY ON
        QT_NO_WIN32_MANIFEST ON
    )
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
