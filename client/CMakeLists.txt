find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

qt_add_executable(feather-chat 
    src/main.cpp
)

# app.rc embeds app.manifest directly as a Win32 resource (RT_MANIFEST id=1),
# bypassing Qt's broken manifest generator which emits an invalid
# <requestedExecutionLevel /> tag (missing the required "level" attribute)
# causing a Windows SxS launch failure.
# QT_NO_WIN32_MANIFEST prevents Qt from embedding its own conflicting manifest.
# QT_NO_DEPLOY skips windeployqt (missing qtpaths in static vcpkg builds).
if(WIN32)
    target_sources(feather-chat PRIVATE app.rc)
    set_target_properties(feather-chat PROPERTIES 
        QT_NO_DEPLOY ON
        QT_NO_WIN32_MANIFEST ON
    )
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
