find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

qt_add_executable(feather-chat 
    src/main.cpp
)

# QT_NO_DEPLOY skips windeployqt: qtpaths is missing in static vcpkg builds.
#
# Qt/vcpkg generates a broken <requestedExecutionLevel /> tag (missing the
# required "level" attribute) that causes a Windows SxS launch failure.
# QT_NO_WIN32_MANIFEST is ignored by this Qt build, so we let Qt embed its
# broken manifest and then overwrite it post-link with mt.exe using our own
# correct app.manifest.
if(WIN32)
    set_target_properties(feather-chat PROPERTIES QT_NO_DEPLOY ON)

    add_custom_command(TARGET feather-chat POST_BUILD
        COMMAND mt.exe
            -manifest "${CMAKE_CURRENT_SOURCE_DIR}/app.manifest"
            -outputresource:"$<TARGET_FILE:feather-chat>;#1"
        COMMENT "Re-embedding corrected Windows manifest (workaround for Qt/vcpkg bug)"
        VERBATIM
    )
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
