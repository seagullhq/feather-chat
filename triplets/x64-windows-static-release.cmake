# Like the built-in x64-windows-static, minus the debug build of every
# dependency. Halves both the first-build time and the disk footprint.
#
# The debug halves only matter if you want to step inside Qt itself; stepping
# into feather-chat's own code works regardless, since that is built by the
# preset, not by vcpkg.
set(VCPKG_TARGET_ARCHITECTURE x64)
set(VCPKG_CRT_LINKAGE static)
set(VCPKG_LIBRARY_LINKAGE static)
set(VCPKG_BUILD_TYPE release)
