# Build AInode for Windows x64: the NSIS installer, checked.
#
#   powershell -ExecutionPolicy Bypass -File app\scripts\release-windows.ps1 -BuildDistro AInode-build
#
# Windows has no single shell with the whole toolchain (the easyNode lesson):
# the engine is staged INSIDE a WSL build distribution (Ubuntu 24.04+ with the
# Linux build tools, Rust with the musl target, unzip) by
# stage-engine-windows.sh, and the app is built here with the Windows toolchain
# (Node 20+, Rust MSVC, Visual Studio Build Tools). Never the user's own
# distributions, and never AInode's own runtime distribution `AInode`.
#
# Proof before anything is printed as done: every file under the built
# resources\engine equals its sha256 in MANIFEST.json (the NSIS bundler packs
# those bytes as they are; unlike linuxdeploy it patches nothing).
#
# Authenticode (security audit M2, docs/SIGNING.md "Windows"): with a sign
# command, from -SignCommand or $env:AINODE_SIGN_COMMAND, Tauri signs
# ainode.exe, the uninstaller and the installer with it (`%1` is the file),
# and this script refuses to finish unless the installer carries a valid
# signature and the ainode.exe packed in it one by the same certificate. The engine's files keep the
# bytes MANIFEST.json pins (llama-server.exe as built upstream). Without one
# the build is UNSIGNED, said at the end. -AllowUntrusted accepts a signature
# whose certificate this PC does not trust (a test certificate), never for a
# release.
param(
  [Parameter(Mandatory = $true)][string]$BuildDistro,
  [string]$Out = "",
  [string]$SignCommand = $env:AINODE_SIGN_COMMAND,
  [switch]$AllowUntrusted
)
$ErrorActionPreference = 'Stop'
$app = Split-Path -Parent $PSScriptRoot
$tauri = Join-Path $app 'src-tauri'
if (-not $Out) { $Out = Join-Path $app 'dist-windows' }
if ($BuildDistro -eq 'AInode') { throw "AInode is the app's own runtime distribution; build in another one" }

# The checkout as WSL sees it.
$full = (Resolve-Path $app).Path
$linux = '/mnt/' + $full.Substring(0, 1).ToLower() + ($full.Substring(2) -replace '\\', '/')

Write-Host "staging the engine in WSL ($BuildDistro)"
wsl.exe -d $BuildDistro --cd / -- bash -lc "export PATH=`$HOME/.cargo/bin:`$PATH; bash '$linux/scripts/stage-engine-windows.sh'"
if ($LASTEXITCODE -ne 0) { throw "stage-engine-windows.sh failed" }

# A copy left in target\ from an earlier build is replaced, never merged.
Remove-Item -Recurse -Force (Join-Path $tauri 'target\release\resources') -ErrorAction SilentlyContinue

Push-Location $app
try {
  npm ci --no-audit --no-fund --prefer-offline
  if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
  if ($SignCommand) {
    if ($SignCommand -notmatch '%1') { throw "the sign command must name the file as %1" }
    # A file, not a JSON argument: Windows PowerShell drops the quotes inside
    # an argument to a native program (seen: "key must be a string").
    $config = Join-Path $env:TEMP ('ainode-sign-' + [guid]::NewGuid().ToString('N') + '.json')
    @{ bundle = @{ windows = @{ signCommand = $SignCommand } } } | ConvertTo-Json -Compress -Depth 5 | Set-Content -Encoding ascii $config
    try { npx tauri build --config $config } finally { Remove-Item $config -ErrorAction SilentlyContinue }
  } else {
    npx tauri build
  }
  if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
} finally { Pop-Location }

# Prove the engine the installer carries: every file listed in MANIFEST.json present with its
# sha256, and nothing beside them (a stray DLL next to llama-server.exe would be loaded first).
function Test-Engine([string]$dir, [string]$label) {
  $manifest = Get-Content (Join-Path $dir 'MANIFEST.json') -Raw | ConvertFrom-Json
  if ($manifest.platform -ne 'windows-x86_64') { throw "$label MANIFEST.json is for $($manifest.platform), not windows-x86_64" }
  $bad = @()
  $listed = @{}
  foreach ($f in $manifest.files) {
    $rel = $f.path -replace '/', '\'
    $listed[$rel] = $true
    $p = Join-Path $dir $rel
    if (-not (Test-Path $p)) { $bad += "missing $($f.path)"; continue }
    if ((Get-FileHash -Algorithm SHA256 $p).Hash.ToLower() -ne $f.sha256) { $bad += "changed $($f.path)" }
  }
  if ($bad) { throw ("$label is not the staged engine: " + ($bad -join '; ')) }
  $extra = Get-ChildItem $dir -Recurse -File | ForEach-Object { $_.FullName.Substring($dir.Length + 1) } |
    Where-Object { -not $listed[$_] -and $_ -ne 'MANIFEST.json' -and $_ -ne '.keep' }
  if ($extra) { throw ("files in $label that MANIFEST.json does not list: " + ($extra -join ', ')) }
  Write-Host "$label`: $($manifest.files.Count) engine files identical to MANIFEST.json, nothing beside them"
  return $manifest
}
$engine = Join-Path $tauri 'target\release\resources\engine'
$manifest = Test-Engine $engine 'the built engine'
if (Get-ChildItem $engine -Recurse -Filter 'devbridge*') { throw "a dev bridge is in the bundle" }

$setup = Get-ChildItem (Join-Path $tauri 'target\release\bundle\nsis\*-setup.exe') | Sort-Object LastWriteTime | Select-Object -Last 1

# Signed means signed: the installer trusted here, and the ainode.exe packed in
# it by the same certificate. Tauri puts the unsigned build back in
# target\release after packing, so the packed copy is the one to read
# (nsis-packed-signed.py: reads the payload, runs nothing).
# -AllowUntrusted lets through only a chain this PC does not trust (UnknownError
# or NotTrusted, as a test certificate gives), never a broken signature
# (HashMismatch: the file changed after signing) or any other status.
function Test-SignatureStatus([string]$status, [bool]$allowUntrusted) {
  return ($status -eq 'Valid') -or ($allowUntrusted -and ($status -eq 'UnknownError' -or $status -eq 'NotTrusted'))
}
function Test-Signed([string]$path) {
  $s = Get-AuthenticodeSignature $path
  if (-not $s.SignerCertificate) { throw "$path carries no Authenticode signature" }
  if (-not (Test-SignatureStatus ([string]$s.Status) ([bool]$AllowUntrusted))) { throw "$path`: signature $($s.Status): $($s.StatusMessage)" }
  Write-Host "signed: $(Split-Path -Leaf $path) by $($s.SignerCertificate.Subject) ($($s.Status))"
  return $s.SignerCertificate
}
$signed = $false
if ($SignCommand) {
  $cert = Test-Signed $setup.FullName
  $der = Join-Path $env:TEMP ('ainode-signer-' + [guid]::NewGuid().ToString('N') + '.der')
  [IO.File]::WriteAllBytes($der, $cert.RawData)
  try {
    python -I (Join-Path $PSScriptRoot 'nsis-packed-signed.py') $setup.FullName (Join-Path $tauri 'target\release\ainode.exe') $der
    if ($LASTEXITCODE -ne 0) { throw "the ainode.exe in the installer is not signed by the installer's certificate" }
  } finally { Remove-Item $der -ErrorAction SilentlyContinue }
  $signed = $true
}
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item $setup.FullName $Out -Force
$sha = (Get-FileHash -Algorithm SHA256 (Join-Path $Out $setup.Name)).Hash.ToLower()
"$sha  $($setup.Name)" | Set-Content -Encoding ascii (Join-Path $Out 'SHA256SUMS')

# The packed installer itself, not only the folder the bundler packed: a silent install into a
# scratch folder, the engine in there checked the same way, then its own uninstaller. Skipped when
# AInode is installed on this machine or its WSL distribution exists (the probe's uninstall would
# take that install's uninstall entry and the distribution with it; tools/win-install-test.ps1
# covers that case).
$installed = Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue |
  Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq 'AInode' }
$distro = ((wsl.exe --list --quiet) -replace "`0", "") -contains 'AInode'
if ($installed -or $distro) {
  Write-Host "AInode is installed here (or its distribution exists): the packed installer is checked by tools/win-install-test.ps1 instead"
} else {
  $probe = Join-Path $env:TEMP ('ainode-packed-' + [guid]::NewGuid().ToString('N'))
  Start-Process -FilePath (Join-Path $Out $setup.Name) -ArgumentList '/S', "/D=$probe" -Wait
  if (-not (Test-Path (Join-Path $probe 'ainode.exe'))) { throw "the packed installer did not install into $probe" }
  try {
    $null = Test-Engine (Join-Path $probe 'resources\engine') 'the packed installer'
    if ($signed) { $null = Test-Signed (Join-Path $probe 'ainode.exe') }
  } finally {
    # NSIS uninstallers copy themselves away and return at once; `_?=<dir>` runs this one in
    # place, so -Wait waits for the real uninstall (the uninstall entry gone with it). Without
    # it, the entry kept the scratch path and the next silent install landed there (seen).
    Start-Process -FilePath (Join-Path $probe 'uninstall.exe') -ArgumentList '/S', "_?=$probe" -Wait
    Remove-Item -Recurse -Force $probe -ErrorAction SilentlyContinue
    # The installer remembers its last folder in HKCU:\Software\<publisher>\AInode and offers it to
    # the next install (RestorePreviousInstallLocation). The probe's folder must not be it.
    foreach ($k in Get-ChildItem 'HKCU:\Software' -ErrorAction SilentlyContinue) {
      $pk = Join-Path $k.PSPath 'AInode'
      if ((Test-Path $pk) -and ((Get-ItemProperty $pk -ErrorAction SilentlyContinue).'(default)' -like "$probe*")) {
        Remove-Item -Path $pk -Recurse -Force
        Write-Host "forgot the probe folder as the remembered install location ($($k.PSChildName))"
      }
    }
  }
  if (Test-Path (Join-Path $probe 'ainode.exe')) { throw "the probe install did not uninstall" }
  Write-Host "the packed installer installed, checked and uninstalled again"
}
Write-Host ""
if (-not $signed) { Write-Host "UNSIGNED: no -SignCommand or AINODE_SIGN_COMMAND (docs/SIGNING.md, Windows); SmartScreen will warn" }
Write-Host "AInode for Windows x64:"
Write-Host "$sha  $($setup.Name)"
