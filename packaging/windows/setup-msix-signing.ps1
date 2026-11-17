# One-time setup for signing the Windows .msi and .msix.
#  1. creates a self-signed code-signing certificate (CN=Feather Chat)
#  2. saves the PFX and CER under %USERPROFILE%\feather-chat-signing (not in the repo)
#  3. trusts the certificate on this PC so the MSIX can be installed
#  4. prints the GitHub secrets and variables to add by hand (nothing is sent to GitHub)
#
# Run in an elevated PowerShell:
#   powershell -ExecutionPolicy Bypass -File packaging\windows\setup-msix-signing.ps1
param(
  [string]$Subject = "CN=Feather Chat"
)
$ErrorActionPreference = "Stop"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run this script in an elevated (Administrator) PowerShell."
}

$dir = Join-Path $env:USERPROFILE "feather-chat-signing"
New-Item -ItemType Directory -Force $dir | Out-Null
$pfxPath = Join-Path $dir "feather-sign.pfx"
$cerPath = Join-Path $dir "feather-sign.cer"

$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject `
  -KeyUsage DigitalSignature -KeyAlgorithm RSA -KeyLength 3072 `
  -NotAfter (Get-Date).AddYears(3) -CertStoreLocation "Cert:\CurrentUser\My"

# Random 32-char password for the PFX
$chars = (48..57) + (65..90) + (97..122)
$pass = -join ($chars | Get-Random -Count 32 | ForEach-Object { [char]$_ })
$secure = ConvertTo-SecureString $pass -AsPlainText -Force

Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $secure | Out-Null
Export-Certificate -Cert $cert -FilePath $cerPath | Out-Null

# Trust it on this PC (MSIX installs need the publisher cert in TrustedPeople)
Import-Certificate -FilePath $cerPath -CertStoreLocation "Cert:\LocalMachine\TrustedPeople" | Out-Null

$b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($pfxPath))

# The two GitHub secrets, written to files to copy into the repo settings
$b64File = Join-Path $dir "WINDOWS_SIGN_PFX_BASE64.txt"
$passFile = Join-Path $dir "WINDOWS_SIGN_PFX_PASSWORD.txt"
Set-Content -Path $b64File -Value $b64 -NoNewline -Encoding ASCII
Set-Content -Path $passFile -Value $pass -NoNewline -Encoding ASCII

Write-Host ""
Write-Host "=== WINDOWS_SIGN_PFX_BASE64 (secret) ===" -ForegroundColor Yellow
Write-Host $b64
Write-Host ""
Write-Host "=== WINDOWS_SIGN_PFX_PASSWORD (secret) ===" -ForegroundColor Yellow
Write-Host $pass
Write-Host ""
Write-Host "Files in ${dir}:" -ForegroundColor Yellow
Write-Host "  WINDOWS_SIGN_PFX_BASE64.txt   -> secret WINDOWS_SIGN_PFX_BASE64"
Write-Host "  WINDOWS_SIGN_PFX_PASSWORD.txt -> secret WINDOWS_SIGN_PFX_PASSWORD"
Write-Host "  feather-sign.cer              -> public cert, already trusted on this PC"
Write-Host ""
Write-Host "Repo variables to add by hand (Settings > Secrets and variables > Actions > Variables):"
Write-Host "  MSIX_PUBLISHER      = $Subject"
Write-Host "  MSIX_IDENTITY_NAME  = com.feather.chat"
Write-Host ""
Write-Host "Keep feather-sign.pfx and the two .txt files private. Delete the .txt files after pasting them into GitHub."
Write-Host "Then re-run the build workflow and install with: Add-AppxPackage -Path <file>.msix"
