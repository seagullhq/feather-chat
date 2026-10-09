cmake_minimum_required(VERSION 3.21)
project(feather-chat LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Qt comes from vcpkg, which builds it with MSVC. Linking it against anything
# else is an ABI mismatch that shows up as link errors or a crash at startup,
# and Ninja happily picks up clang-cl/clang++ from LLVM's bin directory when
# that happens to precede the MSVC one on PATH. Fail here, with the reason,
# instead of half a build later. scripts/msvc-env.ps1 fixes the PATH for you.
if(WIN32 AND NOT CMAKE_CXX_COMPILER_ID STREQUAL "MSVC")
    message(FATAL_ERROR
        "The Windows build needs MSVC, but CMake selected "
        "'${CMAKE_CXX_COMPILER_ID}' (${CMAKE_CXX_COMPILER}).\n"
        "Qt is built by vcpkg with MSVC, so the two must match.\n"
        "Either configure from a Developer PowerShell for Visual Studio, or run "
        "the build through `just`, which loads the MSVC environment for you.")
endif()

add_subdirectory(client)
