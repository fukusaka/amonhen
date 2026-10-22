<#
Downloads the engine (CLI and daemon) from GitHub Releases and installs it (Windows; install-engine.sh elsewhere).
`amonhen.exe install` puts it in %LOCALAPPDATA%\amonhen, points `current` at it and copies it to bin\amonhen.exe.
  scripts\install-engine.ps1 [[-Release] stable|snapshot|<tag>] [-Verify auto|require|skip] [-Force] [-NoLink]
                             [-KeepDownload <dir>]
Without a clone:
  irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1 | iex
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1))) snapshot
-Verify: auto checks the build provenance attestation when gh is logged in; require stops when it can't be checked.
Runs on Windows PowerShell 5.1 and PowerShell 7. It is also run through iex, so failures throw instead of exiting.
Keep this file ASCII: PowerShell 5.1 reads a UTF-8 file without a BOM in the ANSI code page.
#>
param(
    [Parameter(Position = 0)][string]$Release = 'stable',
    [ValidateSet('auto', 'require', 'skip')][string]$Verify = 'auto',
    [switch]$Force,
    [switch]$NoLink,
    [string]$KeepDownload
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # 5.1's Invoke-WebRequest is slow while drawing progress
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$repo = 'fukusaka/amonhen'
$api = "https://api.github.com/repos/$repo/releases"

# Runs a native command and returns its exit code (in 5.1, its stderr would stop the script under Stop).
function Invoke-Native([scriptblock]$Command) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Command | Out-Host } finally { $ErrorActionPreference = $saved }
    $LASTEXITCODE
}

# The OS's CPU, even when an x64 PowerShell runs emulated on an ARM machine.
$arch = $null
try { $arch = [string][Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch {}
if (-not $arch) { $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE } }
$cpu = switch -regex ($arch) {
    '^(X64|AMD64)$' { 'x86_64' }
    '^(Arm64|ARM64)$' { 'aarch64' }
    default { throw "unsupported CPU: $arch" }
}

# Stable releases of the app and engine are tagged v<number> (the VSCode extension's are vscode-v*). Newest first.
$headers = @{ 'User-Agent' = 'amonhen-install-engine' }
$pattern = "^amonhen-.+-windows-$cpu\.zip$"
$uri = if ($Release -eq 'stable') { "$api`?per_page=100" } else { "$api/tags/$Release" }
try { $releases = Invoke-RestMethod -Uri $uri -Headers $headers } catch { throw "no release $Release found ($($_.Exception.Message))" }
# Through a variable: 5.1's Invoke-RestMethod emits a JSON array as one object
$releases = @($releases)
if ($Release -eq 'stable') { $releases = $releases | Where-Object { $_.tag_name -match '^v\d' -and -not $_.prerelease -and -not $_.draft } }
$asset = $null
foreach ($r in $releases) {
    $asset = $r.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
    if ($asset) { $tag = $r.tag_name; break }
}
if (-not $asset) { throw "no $Release engine for windows-$cpu found" }

$work = Join-Path ([IO.Path]::GetTempPath()) "amonhen-install-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Join-Path $work $asset.name
    Write-Host "downloading $($asset.browser_download_url)"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing -Headers $headers

    $gh = Get-Command gh -ErrorAction SilentlyContinue
    $canVerify = $gh -and (Invoke-Native { gh auth status 2>$null }) -eq 0
    if ($Verify -eq 'skip') {
        Write-Host 'attestation: skipped'
    } elseif ($canVerify) {
        if ((Invoke-Native { gh attestation verify $zip --repo $repo }) -ne 0) { throw "attestation verification failed: $($asset.name)" }
        Write-Host 'attestation: verified'
    } elseif ($Verify -eq 'require') {
        throw 'attestation: gh is missing or not logged in (gh auth login)'
    } else {
        Write-Host 'attestation: not verified (needs gh, logged in)'
    }

    $dir = Join-Path $work 'engine'
    Expand-Archive -Path $zip -DestinationPath $dir
    $exe = Join-Path $dir 'amonhen.exe'
    if (-not (Test-Path $exe)) { throw "amonhen.exe not found in $($asset.name)" }
    $installArgs = @('install')
    if ($Force) { $installArgs += '--force' }
    if ($NoLink) { $installArgs += '--no-link' }
    if ((Invoke-Native { & $exe @installArgs }) -ne 0) { throw 'amonhen install failed' }
    if ($KeepDownload) {
        New-Item -ItemType Directory -Path $KeepDownload -Force | Out-Null
        Copy-Item -Path $zip -Destination $KeepDownload -Force
    }

    $bin = Join-Path $env:LOCALAPPDATA 'amonhen\bin'
    $path = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $NoLink -and -not (($path -split ';') -contains $bin)) {
        Write-Host "To use amonhen from a terminal, add $bin to PATH:"
        Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$bin', 'User')"
    }
    Write-Host "installed: $tag ($($asset.name))"
} finally {
    Remove-Item -Recurse -Force -Path $work -ErrorAction SilentlyContinue
}
