# One-time setup for signing the Windows .msi and .msix.
#  1. creates a self-signed code-signing certificate (CN=Feather Chat)
#  2. saves the PFX and CER under %USERPROFILE%\feather-chat-signing (not in the repo)
#  3. stores the PFX and its password as GitHub secrets, and MSIX_PUBLISHER as a variable
#  4. trusts the certificate on this PC so the MSIX can be installed
#
# Run in an elevated PowerShell, from the repo root. Needs gh logged in as repo admin.
#   powershell -ExecutionPolicy Bypass -File packaging\windows\setup-msix-signing.ps1
param(
  [string]$Subject = "CN=Feather Chat",
  [string]$Repo = "seagullhq/feather-chat"
)
$ErrorActionPreference = "Stop"

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw "Run this script in an elevated (Administrator) PowerShell."
}
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw "gh (GitHub CLI) not found" }

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
gh secret set WINDOWS_SIGN_PFX_BASE64 -R $Repo --body $b64
gh secret set WINDOWS_SIGN_PFX_PASSWORD -R $Repo --body $pass
gh variable set MSIX_PUBLISHER -R $Repo --body $Subject
gh variable set MSIX_IDENTITY_NAME -R $Repo --body "com.feather.chat"

Write-Host ""
Write-Host "Done. Publisher: $Subject"
Write-Host "PFX and CER are in $dir (keep the PFX private, never commit it)."
Write-Host "Next: re-run the build workflow. Then install the .msix with: Add-AppxPackage -Path <file>.msix"
