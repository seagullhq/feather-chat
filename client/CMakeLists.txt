find_package(Qt6 REQUIRED COMPONENTS Widgets)

qt_standard_project_setup()

qt_add_executable(feather-chat
    src/main.cpp
)

# QT_NO_DEPLOY skips windeployqt (qtpaths is missing in static vcpkg builds).
#
# Windows manifest: with this toolchain (Clang + lld-link) and Qt's own
# auto-generated manifest, the linked exe ends up with a broken RT_MANIFEST
# whose <requestedExecutionLevel /> lacks the mandatory "level" attribute,
# so Windows refuses to start it (SxS error). Qt's generator is not reliable
# here, and adding our own .rc collides with Qt's manifest (duplicate
# resource). So we overwrite the embedded manifest post-link with mt.exe,
# referenced by its CMake-discovered absolute path (mt.exe is not on PATH).
if(WIN32)
    if(NOT CMAKE_MT)
        message(FATAL_ERROR "cmake could not locate mt.exe; install the Windows SDK")
    endif()

    set_target_properties(feather-chat PROPERTIES QT_NO_DEPLOY ON)

    add_custom_command(TARGET feather-chat POST_BUILD
        COMMAND "${CMAKE_MT}" -nologo
            -manifest "${CMAKE_CURRENT_SOURCE_DIR}/app.manifest"
            -outputresource:$<TARGET_FILE:feather-chat>\;\#1
        COMMENT "Re-emitting corrected Windows application manifest (workaround for broken linker default)"
        VERBATIM
    )
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
