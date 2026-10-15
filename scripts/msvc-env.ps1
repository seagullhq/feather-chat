# Puts the MSVC toolchain (cl.exe, link.exe, the Windows SDK) into the current
# PowerShell session. Dot-source it, do not run it:
#
#     . ./scripts/msvc-env.ps1
#
# Why this exists: CMake's Ninja generator takes whichever C++ driver it finds
# first on PATH. LLVM's bin directory usually precedes the MSVC one, so without
# this the client silently builds with clang++ while Qt came from an MSVC vcpkg
# build. Mixing the two is an ABI mismatch, and it also defeats the
# CMAKE_MSVC_RUNTIME_LIBRARY setting in the windows preset.

$ErrorActionPreference = 'Stop'

# Already inside a Developer PowerShell, or vcpkg has put the toolset on PATH
# some other way. Nothing to do, and in particular nothing that can fail.
if (Get-Command cl.exe -ErrorAction SilentlyContinue) {
    return
}

# vswhere is Microsoft's own locator and reads the Visual Studio installer
# registry, so this finds VS wherever it is installed on the machine rather
# than assuming a path.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
    $vswhere = Join-Path $env:ProgramFiles 'Microsoft Visual Studio\Installer\vswhere.exe'
}
if (-not (Test-Path -LiteralPath $vswhere)) {
    throw 'vswhere.exe not found, and cl.exe is not on PATH. Install Visual Studio with the "Desktop development with C++" workload, or build from a Developer PowerShell.'
}

# -products * covers both Visual Studio and the standalone Build Tools, which
# ship the same C++ workload under different component sets. -requires then
# skips any install that has no x64 toolset, so we never pick an IDE-only one.
$installPath = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
if (-not $installPath) {
    throw 'No Visual Studio installation with the MSVC toolchain was found.'
}
$installPath = $installPath.Trim()

$devcmd = Join-Path $installPath 'Common7\Tools\VsDevCmd.bat'
if (-not (Test-Path -LiteralPath $devcmd)) {
    throw "VsDevCmd.bat not found at $devcmd."
}

# VsDevCmd.bat also repoints VCPKG_ROOT at the vcpkg bundled with Visual Studio,
# and that lands in the dump below. The windows preset reads $env{VCPKG_ROOT} to
# find the toolchain file, so letting the swap through would silently build the
# repo against a different vcpkg than the one it was set up with. Remember what
# the caller had and put it back afterwards.
$vcpkgRootBefore = $env:VCPKG_ROOT

# VsDevCmd.bat only exports into a cmd session, and separate cmd.exe calls do not
# share state. So run it and dump the resulting environment in one go, then copy
# the variables we care about into this PowerShell process.
$devcmdArgs = '"' + $devcmd + '" -no_logo -arch=x64 -host_arch=x64'
$dump = cmd.exe /s /c ($devcmdArgs + ' >nul && set')
if ($LASTEXITCODE -ne 0) {
    throw "VsDevCmd.bat failed for $installPath (exit $LASTEXITCODE)."
}

# Import everything rather than a hand-picked list: the toolchain needs INCLUDE,
# LIB, LIBPATH and a PATH that resolves cl/link, and the SDK vars come with it.
foreach ($line in $dump) {
    if ($line -match '^([^=]+)=(.*)$') {
        [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
    }
}

if ($vcpkgRootBefore) {
    $env:VCPKG_ROOT = $vcpkgRootBefore
}

if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    throw "cl.exe is still not on PATH after loading $installPath."
}
