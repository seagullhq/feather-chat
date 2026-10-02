# Configure and build the Windows client with MSVC, from any PowerShell:
#
#     .\scripts\windows-build.ps1
#
# `just build` calls this. It is a script rather than three justfile lines
# because `just` runs each recipe line in its own PowerShell process: anything
# line 1 puts on PATH is gone by line 2. So a justfile that loaded
# msvc-env.ps1 and then called cmake would load the MSVC toolchain and throw it
# away before cmake ran, and CMake's Ninja generator would fall back to
# whichever C++ driver came first on PATH, usually clang++ from LLVM.

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$buildDir = Join-Path $repoRoot 'dist/client-windows'

. (Join-Path $PSScriptRoot 'msvc-env.ps1')

# vcpkg reads its list of binary caches from the environment. Leaving
# VCPKG_BINARY_SOURCES in the preset's cacheVariables achieves nothing: CMake
# passes it to the toolchain as an ordinary cache entry, vcpkg never consults
# it, and CMake warns the variable went unused. vcpkg then falls back to its own
# per-machine cache, so a fresh clone pays the full Qt build every time.
#
# `clear` drops that per-machine default, which is the point: the cache the repo
# ships in .cache is what a teammate or a CI runner can be pointed at.
$binaryCache = (Join-Path $repoRoot '.cache/vcpkg-binary') -replace '\\', '/'
$env:VCPKG_BINARY_SOURCES = "clear;files,$binaryCache,readwrite"

# The windows preset has its own binary dir, so the default preset's system-Qt
# build cannot poison this one. But CMake still remembers the compiler it chose
# on the first configure here and reuses it on every later run, whatever PATH
# offers by then. So an IDE configure, or a `cmake --preset windows` typed
# without the MSVC environment, keeps clang++ pinned and every MSVC build
# afterwards fails in CMakeLists.txt with the compiler complaint, no matter how
# correct the environment is. Detect that and clear it.
#
# Only CMakeCache.txt and CMakeFiles/ are removed. vcpkg_installed/ holds the
# installed dependencies, Qt above all. Dropping it makes the next configure
# build them from source again, which costs an hour unless the binary cache in
# .cache/vcpkg-binary still has Qt's zips in it.
$cache = Join-Path $buildDir 'CMakeCache.txt'
if (Test-Path -LiteralPath $cache) {
    $match = Select-String -LiteralPath $cache -Pattern '^CMAKE_CXX_COMPILER:FILEPATH=(.+)$'
    if ($match) {
        $cached = $match.Matches[0].Groups[1].Value.Trim()
        if ($cached -notmatch 'MSVC') {
            Write-Host "CMake cache pins '$cached', which is not MSVC. Clearing it so CMake re-detects the compiler."
            Remove-Item -LiteralPath $cache -Force
            Remove-Item -LiteralPath (Join-Path $buildDir 'CMakeFiles') -Recurse -Force
        }
    }
}

Push-Location $repoRoot
try {
    cmake --preset windows
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    cmake --build --preset windows
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}
