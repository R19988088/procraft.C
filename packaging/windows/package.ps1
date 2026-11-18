<#
.SYNOPSIS
  Packages the desktop app for Windows x64, or Windows on ARM.
.DESCRIPTION
  Writes, into $env:DIST (default: dist\), for $env:ARCH (x64, the default, or arm64):
    procraft-<version>-windows-<arch>.msi            the installer (WiX v5), for every user
    procraft-<version>-windows-<arch>-portable.zip   ProCraft.exe and the licences, to run anywhere
  Both hold Microsoft's DirectX Shader Compiler beside ProCraft.exe (dxcompiler.dll and dxil.dll),
  which Direct3D 12 makes the shaders with, from its release on GitHub (see DXC below; $env:DXC_ZIP
  can name the zip, downloaded already).
  An arm64 build can be made on an x64 PC with Visual Studio's ARM64 build tools.
  The C runtime is linked in (+crt-static), so nothing else needs installing. The version is
  $env:VERSION, else procraft.version in android\gradle.properties (the one a release is tagged
  with). Needs Rust (MSVC) and WiX v5 with its UI and Util extensions ($env:WIX_EXE can name another
  wix.exe):
    dotnet tool install --global wix --version 5.0.2
    wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2
  Modelled on SolidCraft's packaging (tangency-labs/solidcraft).
.EXAMPLE
  pwsh packaging/windows/package.ps1
  pwsh packaging/windows/package.ps1 -SkipBuild
  $env:ARCH = 'arm64'; pwsh packaging/windows/package.ps1
#>
param([switch] $SkipBuild)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

$Version = $env:VERSION
if (-not $Version) {
  $line = Get-Content (Join-Path $Root 'android\gradle.properties') | Where-Object { $_ -match '^procraft\.version=' } | Select-Object -First 1
  $Version = ($line -replace '^procraft\.version=', '').Trim()
}
if (-not $Version) { throw 'no version: set VERSION or procraft.version in android\gradle.properties' }
# the MSI's own version is numbers only: a beta's suffix is left out
$MsiVersion = ($Version -split '-')[0]
# the machine it is for: its Rust target and the PE header's machine number
$Arch = if ($env:ARCH) { $env:ARCH } else { 'x64' }
if ($Arch -eq 'x64') {
  $Target = 'x86_64-pc-windows-msvc'
  $PeMachine = 0x8664
} elseif ($Arch -eq 'arm64') {
  $Target = 'aarch64-pc-windows-msvc'
  $PeMachine = 0xAA64
} else {
  throw "ARCH must be x64 or arm64, not $Arch"
}
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
# WIX_EXE, not WIX: Windows runners set WIX to an old WiX 3's folder
$Wix = if ($env:WIX_EXE) { $env:WIX_EXE } else { 'wix' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Write-Output "ProCraft $Version for Windows $Arch"

if (-not $SkipBuild) {
  # the C runtime linked in; only for this target, so build scripts and proc-macros are as ever
  Set-Item -Path ('env:CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS') -Value '-C target-feature=+crt-static'
  Invoke-Native 'cargo build' { cargo build --release --locked -p procraft-desktop --target $Target }
}
$Exe = Join-Path $TargetDir "$Target\release\procraft.exe"

# the .exe: for this machine, a windowed program (no console), and needing no Visual C++ runtime
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$pe = [BitConverter]::ToInt32($bytes, 0x3C)
$machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
$subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C)
if ($machine -ne $PeMachine) { throw ('procraft.exe is for machine 0x{0:X}, not {1}' -f $machine, $Arch) }
if ($subsystem -ne 2) { throw "procraft.exe has PE subsystem $subsystem, not 2 (a window)" }
if ([System.Text.Encoding]::ASCII.GetString($bytes).Contains('VCRUNTIME140.dll')) {
  throw 'procraft.exe needs VCRUNTIME140.dll: the C runtime should be linked in (+crt-static)'
}
Write-Output "ok procraft.exe: $Arch, a window, no Visual C++ runtime needed"

$Stage = Join-Path $TargetDir 'windows-package\stage'
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item $Exe (Join-Path $Stage 'ProCraft.exe')
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md') { Copy-Item (Join-Path $Root $f) $Stage }

# ---- DXC ----------------------------------------------------------------------------------------
# Microsoft's DirectX Shader Compiler: dxcompiler.dll, and dxil.dll, which signs what it makes.
# Direct3D 12 makes the shaders with it in place of the older FXC, where they are beside the .exe
# (crates/gpu/src/compiler.rs). From the release pinned here, checked against its SHA-256; with
# their licences as the release gives them (packaging\windows\dxc keeps the LLVM and MIT texts).
$DxcRelease = 'v1.9.2609'
$DxcZipName = 'dxc_2026_09_29.zip'
$DxcSha256 = 'ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1'
$DxcZip = if ($env:DXC_ZIP) { $env:DXC_ZIP } else { Join-Path $TargetDir "windows-package\$DxcZipName" }
if (-not (Test-Path $DxcZip)) {
  Write-Output "==> download DXC $DxcRelease"
  $ProgressPreference = 'SilentlyContinue'
  Invoke-WebRequest -OutFile $DxcZip `
    -Uri "https://github.com/microsoft/DirectXShaderCompiler/releases/download/$DxcRelease/$DxcZipName"
}
$sum = (Get-FileHash $DxcZip -Algorithm SHA256).Hash.ToLower()
if ($sum -ne $DxcSha256) { throw "$DxcZip has SHA-256 $sum, not $DxcSha256 (DXC $DxcRelease)" }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$dxcArchive = [System.IO.Compression.ZipFile]::OpenRead($DxcZip)
try {
  # the zip's paths are written with backslashes
  $entry = {
    param([string] $Path)
    $found = $dxcArchive.Entries | Where-Object { ($_.FullName -replace '\\', '/') -eq $Path } | Select-Object -First 1
    if (-not $found) { throw "$DxcZipName has no $Path" }
    $found
  }
  foreach ($dll in 'dxcompiler.dll', 'dxil.dll') {
    $to = Join-Path $Stage $dll
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile((& $entry "bin/$Arch/$dll"), $to, $true)
    $bytes = [System.IO.File]::ReadAllBytes($to)
    $machine = [BitConverter]::ToUInt16($bytes, [BitConverter]::ToInt32($bytes, 0x3C) + 4)
    if ($machine -ne $PeMachine) { throw ('{0} is for machine 0x{1:X}, not {2}' -f $dll, $machine, $Arch) }
  }
  foreach ($licence in 'LICENSE-LLVM.txt', 'LICENCE-MIT.txt', 'LICENSE-MS.txt') {
    $to = Join-Path $Stage "DXC-$licence"
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile((& $entry $licence), $to, $true)
  }
  # the texts the app and NOTICE give are the release's (Microsoft's terms for dxil.dll are only
  # in the packages, as the release gives them)
  foreach ($licence in 'LICENSE-LLVM.txt', 'LICENCE-MIT.txt') {
    $ours = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "dxc\$licence")) -replace "`r", ''
    if (([System.IO.File]::ReadAllText((Join-Path $Stage "DXC-$licence")) -replace "`r", '') -ne $ours) {
      throw "packaging\windows\dxc\$licence isn't DXC $DxcRelease's $licence"
    }
  }
} finally {
  $dxcArchive.Dispose()
}
Write-Output "ok DXC $DxcRelease ($Arch): dxcompiler.dll and dxil.dll beside ProCraft.exe"

# ---- MSI ----------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "procraft-$Version-windows-$Arch.msi"
Invoke-Native 'wix build' {
  & $Wix build (Join-Path $PSScriptRoot 'procraft.wxs') -arch $Arch `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext -culture en-US `
    -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$Stage" `
    -d "IconPath=$(Join-Path $Root 'assets\app-icon\procraft.ico')" -o $Msi
}
Invoke-Native 'wix msi validate' {
  & $Wix msi validate $Msi -intermediateFolder (Join-Path $TargetDir 'windows-package\msi-validation')
}
# its debug symbols (.wixpdb) aren't for the release
Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Msi, '.wixpdb'))

# ---- portable zip -------------------------------------------------------------------------------
$Name = "procraft-$Version-windows-$Arch-portable"
$Portable = Join-Path $TargetDir "windows-package\$Name"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
Copy-Item -Recurse $Stage $Portable
$Zip = Join-Path $Dist "$Name.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

Get-Item $Msi, $Zip | Format-Table Name, Length
