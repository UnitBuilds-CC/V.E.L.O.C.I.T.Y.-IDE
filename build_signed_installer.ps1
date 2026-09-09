<#
.SYNOPSIS
    Build a code-signed V.E.L.O.C.I.T.Y. Windows installer.

.DESCRIPTION
    One-stop signed-release entry point referenced by CODE_SIGNING.md.
    It delegates the release build + staging to build_release.ps1, then
    compiles installer.iss with Inno Setup (ISCC), signing every embedded
    executable and the final setup bundle with signtool using an RFC 3161
    timestamp, and finally verifies the resulting Authenticode signature.

.PARAMETER CertPath
    Path to the code-signing certificate (.pfx/.p12) including its private key.

.PARAMETER CertPassword
    Password protecting the certificate's private key. Never logged.

.PARAMETER Version
    Installer AppVersion. Defaults to `git describe --tags --always`
    (falling back to 0.1.0-dev), mirroring build_release.ps1.

.PARAMETER TimestampUrl
    RFC 3161 timestamp authority. Defaults to DigiCert.

.PARAMETER SkipBuild
    Skip cargo build; sign the binaries already staged in dist/.

.EXAMPLE
    .\build_signed_installer.ps1 -CertPath "$env:TEMP\cert.pfx" -CertPassword $env:SIGN_CERT_PASSWORD
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$CertPath,

    [Parameter(Mandatory = $true)]
    [string]$CertPassword,

    [string]$Version,

    [string]$TimestampUrl = "http://timestamp.digicert.com",

    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

# ── Validate certificate ──────────────────────────────────────────────
if (-not (Test-Path -LiteralPath $CertPath)) {
    throw "Code-signing certificate not found: $CertPath"
}
$CertPath = (Resolve-Path -LiteralPath $CertPath).Path
if ([string]::IsNullOrEmpty($CertPassword)) {
    throw "CertPassword must not be empty."
}

# ── Resolve version (mirror build_release.ps1) ────────────────────────
if (-not $Version) {
    $Version = (git -C $RepoRoot describe --tags --always 2>$null)
    if (-not $Version) { $Version = "0.1.0-dev" }
    $Version = $Version -replace '^v', ''
}
Write-Host "Building signed installer for V.E.L.O.C.I.T.Y. $Version" -ForegroundColor Cyan

# ── Locate Inno Setup (ISCC.exe) ──────────────────────────────────────
$iscc = $null
foreach ($v in @('7', '6')) {
    foreach ($base in @("${env:ProgramFiles(x86)}", $env:ProgramFiles)) {
        if (-not $base) { continue }
        $candidate = Join-Path $base "Inno Setup $v\ISCC.exe"
        if (Test-Path -LiteralPath $candidate) { $iscc = $candidate; break }
    }
    if ($iscc) { break }
}
if (-not $iscc) {
    throw "Inno Setup (ISCC.exe) not found. Install it from https://jrsoftware.org/isinfo.php, or run .\build_release.ps1 -ArchiveOnly for an unsigned ZIP."
}

# ── Locate signtool.exe (Windows SDK, newest 10.x) ────────────────────
$signtool = $null
$kitRoot = Join-Path "${env:ProgramFiles(x86)}" "Windows Kits\10\bin"
if (Test-Path -LiteralPath $kitRoot) {
    $signtool = Get-ChildItem -LiteralPath $kitRoot -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like '10.*' } |
        Sort-Object -Property Name -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1
}
if (-not $signtool) {
    throw "signtool.exe not found under '$kitRoot'. Install the Windows SDK (10.0.22621.0 or later)."
}

Write-Host "  ISCC:     $iscc"     -ForegroundColor Gray
Write-Host "  signtool: $signtool" -ForegroundColor Gray
Write-Host "  cert:     $CertPath" -ForegroundColor Gray

# ── Build + stage binaries into dist/ (reuse build_release.ps1) ───────
# -ArchiveOnly makes build_release.ps1 build and stage dist/bin while skipping
# its own unsigned-installer step; the signed installer is compiled below.
Write-Host "`n==> Building and staging release binaries..." -ForegroundColor Yellow
$buildArgs = @('-ArchiveOnly')
if ($SkipBuild) { $buildArgs += '-SkipBuild' }
& (Join-Path $RepoRoot 'build_release.ps1') @buildArgs
if ($LASTEXITCODE -ne 0) { throw "build_release.ps1 failed (exit $LASTEXITCODE)." }

# ── Compile + sign the installer ──────────────────────────────────────
$issPath = Join-Path $RepoRoot 'installer.iss'
if (-not (Test-Path -LiteralPath $issPath)) { throw "installer.iss not found at '$issPath'." }

# Inno special sequences: $q -> a literal quote, $f -> the quoted filename of the
# file to sign (required). No literal double-quotes are embedded in this token,
# so PowerShell passes it as a single argument and wraps it for ISCC cleanly
# (avoiding the Windows PowerShell 5.1 embedded-quote mangling).
$signArg = "/Ssigntool=`$q$signtool`$q sign /f `$q$CertPath`$q /p `$q$CertPassword`$q /tr $TimestampUrl /td sha256 /fd sha256 `$f"

Write-Host "`n==> Compiling signed installer with Inno Setup..." -ForegroundColor Yellow
Push-Location $RepoRoot
try {
    & $iscc "/DMyAppVersion=$Version" $signArg $issPath
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup compile/sign failed (exit $LASTEXITCODE)." }
} finally {
    Pop-Location
}

# ── Verify the resulting signature ────────────────────────────────────
$installer = Join-Path $RepoRoot "output\VELOCITY-$Version-Setup.exe"
if (Test-Path -LiteralPath $installer) {
    $sig = Get-AuthenticodeSignature -LiteralPath $installer
    $color = if ($sig.Status -eq 'Valid') { 'Green' } else { 'Yellow' }
    Write-Host "`n==> Signed installer: $installer" -ForegroundColor Green
    Write-Host "    Signature status: $($sig.Status)" -ForegroundColor $color
    if ($sig.SignerCertificate) {
        Write-Host "    Signer:           $($sig.SignerCertificate.Subject)" -ForegroundColor Gray
        Write-Host "    Thumbprint:       $($sig.SignerCertificate.Thumbprint)" -ForegroundColor Gray
    }
    if ($sig.Status -ne 'Valid') {
        Write-Warning "Signature status is '$($sig.Status)', not 'Valid'. Verify the certificate chain and timestamp URL."
    }
} else {
    Write-Warning "Installer compiled but not found at expected path: $installer"
}

Write-Host "`nDone." -ForegroundColor Green
