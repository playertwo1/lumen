param(
  [Parameter(Mandatory = $true)][string]$MsiPath,
  [Parameter(Mandatory = $true)][string]$SetupPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$oldMsiUrl = "https://github.com/playertwo1/lumen/releases/download/windows-v0.7.9/Lumen-Windows-0.7.9.msi"
$oldSetupUrl = "https://github.com/playertwo1/lumen/releases/download/windows-v0.7.9/Lumen-Windows-0.7.9-setup.exe"
$testRoot = [IO.Path]::GetFullPath((Join-Path $env:TEMP "lumen-upgrade-$PID"))
$tempPrefix = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
if (-not $testRoot.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Temporary test directory is outside TEMP: $testRoot"
}

$settingsPath = Join-Path $env:APPDATA "Coucou\settings.json"
$localData = Join-Path $env:LOCALAPPDATA "Coucou"
$hookPath = Join-Path $localData "bin\coucou-hook.exe"
$inboxPath = Join-Path $localData "inbox\synthetic-upgrade.txt"
$logPath = Join-Path $localData "coucou.log"
$identityPath = "HKCU:\Software\Classes\AppUserModelId\fr.louisraille.coucou"

function Assert-Equal([string]$Actual, [string]$Expected, [string]$Name) {
  if ($Actual -cne $Expected) { throw "$Name changed: expected '$Expected', got '$Actual'" }
}

function Invoke-Msi([string]$Path) {
  $process = Start-Process -FilePath "msiexec.exe" -ArgumentList "/i `"$Path`" /qn /norestart" -Wait -PassThru
  if ($process.ExitCode -notin @(0, 3010)) { throw "MSI exited with $($process.ExitCode): $Path" }
}

function Invoke-Nsis([string]$Installer, [string]$Destination) {
  New-Item -ItemType Directory -Path $Destination -Force | Out-Null
  $process = Start-Process -FilePath $Installer -ArgumentList "/S /D=$Destination" -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw "NSIS exited with $($process.ExitCode): $Installer" }
}

function Installed-App([string]$Name) {
  $roots = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
  )
  foreach ($root in $roots) {
    if (-not (Test-Path $root)) { continue }
    foreach ($key in Get-ChildItem -LiteralPath $root) {
      $app = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
      if ($app.PSObject.Properties["DisplayName"] -and $app.DisplayName -eq $Name) { return $app }
    }
  }
  return $null
}

function Seed-LegacyData([string]$Scenario) {
  $marker = "sintetico-$Scenario"
  New-Item -ItemType Directory -Path (Split-Path $settingsPath), (Split-Path $hookPath), (Split-Path $inboxPath) -Force | Out-Null
  $settings = '{"language":"fr","soundVolume":0.17,"syntheticMarker":"' + $marker + '"}'
  [IO.File]::WriteAllText($settingsPath, $settings, [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllBytes($hookPath, [Text.Encoding]::UTF8.GetBytes("synthetic-coucou-hook-$marker"))
  [IO.File]::WriteAllText($inboxPath, $marker, [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText($logPath, $marker, [Text.UTF8Encoding]::new($false))
}

function Assert-LegacyData([string]$Scenario) {
  $marker = "sintetico-$Scenario"
  $settings = '{"language":"fr","soundVolume":0.17,"syntheticMarker":"' + $marker + '"}'
  Assert-Equal ([IO.File]::ReadAllText($settingsPath)) $settings "settings.json"
  Assert-Equal ([Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($hookPath))) "synthetic-coucou-hook-$marker" "legacy hook path"
  Assert-Equal ([IO.File]::ReadAllText($inboxPath)) $marker "legacy inbox"
  Assert-Equal ([IO.File]::ReadAllText($logPath)) $marker "legacy log"
}

try {
  New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
  $oldMsi = Join-Path $testRoot "Lumen-Windows-0.7.9.msi"
  $oldSetup = Join-Path $testRoot "Lumen-Windows-0.7.9-setup.exe"
  Invoke-WebRequest -Uri $oldMsiUrl -OutFile $oldMsi
  Invoke-WebRequest -Uri $oldSetupUrl -OutFile $oldSetup
  Assert-Equal (Get-FileHash -LiteralPath $oldMsi -Algorithm SHA256).Hash.ToLowerInvariant() "9274098dd9d1982524d3e3ee7ec58542faca14c44c799d3fcb3e1bb0748c9e5a" "published 0.7.9 MSI SHA-256"
  Assert-Equal (Get-FileHash -LiteralPath $oldSetup -Algorithm SHA256).Hash.ToLowerInvariant() "74eca2d8e66a39fd1dd5fb76c7fae55196e915c45c2f830f0cf5b3a1e57c181c" "published 0.7.9 NSIS SHA-256"

  Invoke-Msi $oldMsi
  if (-not (Installed-App "Lumen")) { throw "The 0.7.9 MSI did not register Lumen." }
  Seed-LegacyData "msi"
  Invoke-Msi ([IO.Path]::GetFullPath($MsiPath))
  $msiApp = Installed-App "Lumen"
  if (-not $msiApp) { throw "The 0.7.10 MSI did not register Lumen." }
  if ($msiApp.DisplayVersion -ne "0.7.10") { throw "MSI upgrade left version $($msiApp.DisplayVersion) installed." }
  Assert-LegacyData "msi"
  Assert-Equal (Get-ItemProperty -LiteralPath $identityPath).DisplayName "Lumen" "notification display name after MSI upgrade"
  Write-Output "PASS: MSI upgrade updates 0.7.9 to 0.7.10 and preserves synthetic user data."

  $lumenNsiDir = Join-Path $testRoot "lumen-nsis"
  Invoke-Nsis $oldSetup $lumenNsiDir
  $lumenNsiExe = Join-Path $lumenNsiDir "lumen.exe"
  if (-not (Test-Path -LiteralPath $lumenNsiExe)) { throw "The 0.7.9 NSIS install has no lumen.exe in its test directory." }
  if ((Get-Item -LiteralPath $lumenNsiExe).VersionInfo.ProductVersion -ne "0.7.9") { throw "The NSIS baseline is not version 0.7.9." }
  Seed-LegacyData "nsis"
  Invoke-Nsis ([IO.Path]::GetFullPath($SetupPath)) $lumenNsiDir
  if (-not (Test-Path -LiteralPath $lumenNsiExe)) { throw "The Lumen NSIS upgrade removed lumen.exe." }
  if ((Get-Item -LiteralPath $lumenNsiExe).VersionInfo.ProductVersion -ne "0.7.10") { throw "The NSIS upgrade did not install version 0.7.10." }
  Assert-LegacyData "nsis"
  Assert-Equal (Get-ItemProperty -LiteralPath $identityPath).DisplayName "Lumen" "notification display name after NSIS install"
  Write-Output "PASS: NSIS upgrade updates 0.7.9 to 0.7.10 and preserves synthetic user data."
}
finally {
  if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
