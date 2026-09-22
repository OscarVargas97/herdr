<#
.SYNOPSIS
  Builds this herdr fork for Windows and, optionally, Linux; installs or publishes it.

.DESCRIPTION
  Windows builds run inside vcvars64 (cargo cannot find the MSVC linker on its own here).
  Linux builds run in a throwaway rust container (build-linux-docker.sh) because zig needs
  symlinks that Windows only allows with Developer Mode.

  -Install   Copies herdr.exe into every managed herdr release folder. The official
             installer skips same-version installs, and herdr may start from either folder.
  -Linux     Also builds target\fork-dist\herdr-x86_64-unknown-linux-musl.
  -Release   Builds both, then publishes a GitHub release for an already-pushed tag and
             prints the Nix SRI hash of the Linux asset.

.EXAMPLE
  .\scripts\fork\build.ps1 -Install
  .\scripts\fork\build.ps1 -Release v0.9.1-oscar.2
#>
param(
    [switch]$Install,
    [switch]$Linux,
    [string]$Release
)
# No global ErrorActionPreference=Stop: under PowerShell 5.1 it turns any stderr line from
# cargo or docker into a terminating error when the caller redirects output. Native commands
# are checked through $LASTEXITCODE; file operations use -ErrorAction Stop.

$repo = Split-Path (Split-Path $PSScriptRoot)
$dist = Join-Path $repo "target\fork-dist"
$env:Path += ";$env:USERPROFILE\.cargo\bin;C:\Program Files (x86)\Microsoft Visual Studio\Installer;$env:LOCALAPPDATA\Microsoft\WinGet\Packages\zig.zig_Microsoft.Winget.Source_8wekyb3d8bbwe\zig-x86_64-windows-0.16.0"
$vcvars = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
$linuxAsset = "herdr-x86_64-unknown-linux-musl"
$windowsAsset = "herdr-x86_64-pc-windows-msvc.exe"

if ($Release) {
    $Linux = $true
    if (git -C $repo status --porcelain) { throw "Working tree is not clean; commit before releasing." }
    if (-not (git -C $repo ls-remote origin "refs/tags/$Release")) { throw "Tag $Release is not on origin; push it first." }
    if ((git -C $repo rev-parse "$Release^{commit}") -ne (git -C $repo rev-parse HEAD)) { throw "HEAD is not at $Release." }
}
New-Item -ItemType Directory -Force $dist -ErrorAction Stop | Out-Null

Write-Host "Building Windows release..."
cmd /c "`"$vcvars`" >nul && cd /d `"$repo`" && cargo build --release --locked --target x86_64-pc-windows-msvc"
if ($LASTEXITCODE -ne 0) { throw "Windows build failed." }
$exe = Join-Path $repo "target\x86_64-pc-windows-msvc\release\herdr.exe"
Copy-Item $exe (Join-Path $dist $windowsAsset) -Force -ErrorAction Stop

if ($Linux) {
    # Through cmd: redirecting a native command's stderr in PowerShell 5.1 turns it into an error.
    cmd /c "docker info >nul 2>&1"
    if ($LASTEXITCODE -ne 0) { throw "Docker is not running; start Docker Desktop and retry." }
    Write-Host "Building Linux release in a throwaway container..."
    docker run --rm -v "${repo}:/src:ro" -v "${dist}:/out" rust:1.96.1 sh /src/scripts/fork/build-linux-docker.sh
    if ($LASTEXITCODE -ne 0) { throw "Linux build failed." }
}

if ($Install) {
    $stamp = Get-Date -Format "yyyyMMddHHmmss"
    $releases = Join-Path $env:USERPROFILE ".herdr\packages\standalone\releases"
    foreach ($dir in Get-ChildItem $releases -Directory | Where-Object { Test-Path (Join-Path $_.FullName "herdr.exe") }) {
        # Earlier swaps may still be running; they are removed on a later install.
        Get-ChildItem $dir.FullName -Filter "herdr.exe.prev-*" | ForEach-Object {
            try { Remove-Item $_.FullName -Force -ErrorAction Stop } catch {}
        }
        $target = Join-Path $dir.FullName "herdr.exe"
        Rename-Item $target "herdr.exe.prev-$stamp" -ErrorAction Stop
        Copy-Item $exe $target -ErrorAction Stop
        Write-Host "Installed into $($dir.Name)"
    }
    Write-Host "Restart herdr (herdr server stop) to run the new build." -ForegroundColor Yellow
}

if ($Release) {
    $assets = @($windowsAsset, $linuxAsset) | ForEach-Object { Join-Path $dist $_ }
    $sums = $assets | ForEach-Object { "{0} *{1}" -f (Get-FileHash $_ -Algorithm SHA256).Hash.ToLower(), (Split-Path $_ -Leaf) }
    $sumsFile = Join-Path $dist "SHA256SUMS"
    [IO.File]::WriteAllText($sumsFile, (($sums -join "`n") + "`n"))
    gh release create $Release -R OscarVargas97/herdr --title "herdr $Release" --generate-notes @assets $sumsFile
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed." }
    $url = "https://github.com/OscarVargas97/herdr/releases/download/$Release/$linuxAsset"
    Write-Host "Nix hash for the Linux asset:"
    docker run --rm -v "${repo}:/src:ro" nixos/nix sh /src/scripts/fork/nix-hashes.sh $url
}
