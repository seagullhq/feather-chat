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
# resource). So we overwrite the embedded manifest post-link with LLVM's
# llvm-mt (ships with the Clang toolchain, no Windows SDK needed).
if(WIN32)
    set_target_properties(feather-chat PROPERTIES QT_NO_DEPLOY ON)

    # llvm-mt ships with the Clang toolchain (no Windows SDK needed); mt.exe
    # is the MSVC/SDK alternative when present.
    find_program(FEATHER_MT_TOOL NAMES llvm-mt mt)
    if(FEATHER_MT_TOOL)
        add_custom_command(TARGET feather-chat POST_BUILD
            COMMAND "${FEATHER_MT_TOOL}" -nologo
                -manifest "${CMAKE_CURRENT_SOURCE_DIR}/app.manifest"
                -outputresource:$<TARGET_FILE:feather-chat>\;\#1
            COMMENT "Re-emitting corrected Windows application manifest (workaround for broken linker default)"
            VERBATIM
        )
    else()
        message(WARNING
            "No llvm-mt/mt found; the built exe may keep the broken Windows manifest")
    endif()
endif()

target_link_libraries(feather-chat PRIVATE Qt6::Widgets)
