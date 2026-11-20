# Builds dist/packages/feather-chat-windows-x86_64.msix from the release exe.
# Run by `task package-msix` on Windows. Needs the Windows SDK (makeappx).
#
# Optional env:
#   MSIX_IDENTITY_NAME    package identity (default com.feather.chat; use the
#                         reserved name from Partner Center for the Store)
#   MSIX_PUBLISHER        "CN=..." (default CN=Feather Chat; must match the
#                         Partner Center publisher, or the cert subject when signing)
#   MSIX_PUBLISHER_DISPLAY  shown name (default Feather Chat)
#   PFX_B64 / PFX_PASS    code-signing PFX (base64) and password; without them
#                         the package is left unsigned (the Store signs it itself)
param(
  [string]$Exe = "dist/client/release/feather-chat.exe",
  [string]$Icon = "client/assets/icon.png",
  [string]$Out = "dist/packages/feather-chat-windows-x86_64.msix"
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "../..")
Set-Location $root

# Cargo's X.Y.Z becomes the MSIX quad X.Y.Z.0
$cargo = Get-Content client/Cargo.toml -Raw
$ver = [regex]::Match($cargo, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $ver) { throw "version not found in client/Cargo.toml" }
$msixVer = "$ver.0"

$identity = if ($env:MSIX_IDENTITY_NAME) { $env:MSIX_IDENTITY_NAME } else { "com.feather.chat" }
$publisher = if ($env:MSIX_PUBLISHER) { $env:MSIX_PUBLISHER } else { "CN=Feather Chat" }
$publisherDisplay = if ($env:MSIX_PUBLISHER_DISPLAY) { $env:MSIX_PUBLISHER_DISPLAY } else { "Feather Chat" }

$stage = "dist/msix"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/Assets" | Out-Null
Copy-Item $Exe "$stage/feather-chat.exe"

# Tile and store logos, resized from the app icon
Add-Type -AssemblyName System.Drawing
$src = [System.Drawing.Image]::FromFile((Resolve-Path $Icon))
function Save-Logo([int]$w, [int]$h, [string]$path) {
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $g.Clear([System.Drawing.Color]::Transparent)
  $g.DrawImage($src, 0, 0, $w, $h)
  $g.Dispose()
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}
Save-Logo 44 44 "$stage/Assets/Square44x44Logo.png"
Save-Logo 150 150 "$stage/Assets/Square150x150Logo.png"
Save-Logo 50 50 "$stage/Assets/StoreLogo.png"
$src.Dispose()

$manifest = Get-Content packaging/windows/AppxManifest.xml -Raw
$manifest = $manifest.Replace('__IDENTITY_NAME__', $identity).Replace('__PUBLISHER__', $publisher).Replace('__PUBLISHER_DISPLAY__', $publisherDisplay).Replace('__VERSION__', $msixVer)
Set-Content -Path "$stage/AppxManifest.xml" -Value $manifest -Encoding UTF8

$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
if (-not $makeappx) { throw "makeappx.exe not found: install the Windows SDK" }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
Remove-Item $Out -ErrorAction SilentlyContinue
& $makeappx.FullName pack /d $stage /p $Out /o
if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed" }

if ($env:PFX_B64) {
  $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1
  $tmp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
  $pfx = Join-Path $tmp "msix-sign.pfx"
  [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:PFX_B64))
  try {
    & $signtool.FullName sign /fd SHA256 /f $pfx /p $env:PFX_PASS /tr http://timestamp.digicert.com /td SHA256 $Out
    if ($LASTEXITCODE -ne 0) { throw "signtool failed on the msix" }
  } finally {
    Remove-Item $pfx -ErrorAction SilentlyContinue
  }
  Write-Host "signed $Out"
} else {
  Write-Host "unsigned $Out (no PFX_B64)"
}
