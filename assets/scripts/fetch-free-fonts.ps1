# Fetch free-licensed fonts (JetBrains Mono, Inter) into assets/fonts/
# Run from project root: .\assets\scripts\fetch-free-fonts.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Resolve-Path (Join-Path $root "..\..")
$fontsDir = Join-Path $projectRoot "assets\fonts"
New-Item -ItemType Directory -Force -Path $fontsDir | Out-Null

Write-Host "Downloading JetBrains Mono (Apache-2.0)..."
$jbUrl = "https://github.com/JetBrains/JetBrainsMono/releases/latest/download/JetBrainsMono.zip"
$jbZip = Join-Path $fontsDir "jb.zip"
try {
    Invoke-WebRequest -Uri $jbUrl -OutFile $jbZip -UseBasicParsing -ErrorAction Stop
    Expand-Archive -Path $jbZip -DestinationPath $fontsDir -Force
    Remove-Item $jbZip -Force
} catch {
    Write-Warning "Failed to download JetBrains Mono automatically. Please download from https://github.com/JetBrains/JetBrainsMono and place the TTFs under assets\\fonts\\"
}

Write-Host "Downloading Inter (SIL OFL)..."
$interUrl = "https://github.com/rsms/inter/releases/latest/download/Inter.zip"
$interZip = Join-Path $fontsDir "inter.zip"
try {
    Invoke-WebRequest -Uri $interUrl -OutFile $interZip -UseBasicParsing -ErrorAction Stop
    Expand-Archive -Path $interZip -DestinationPath $fontsDir -Force
    Remove-Item $interZip -Force
} catch {
    Write-Warning "Failed to download Inter automatically. Please download from https://github.com/rsms/inter and place the TTFs under assets\\fonts\\"
}

Write-Host "Done. Place JetBrainsMono-*.ttf and Inter-*.ttf into assets/fonts/ if present. The app will prefer these at runtime for deterministic rendering."