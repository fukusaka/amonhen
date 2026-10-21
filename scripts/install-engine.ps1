<#
GitHub Releases からエンジン（CLI と daemon）を取ってきて置く（Windows。install-engine.sh の Windows 版）。
置き場所は %LOCALAPPDATA%\amonhen で、`amonhen.exe install` が `current` を向け、bin\amonhen.exe に写す。
  scripts\install-engine.ps1 [[-Release] stable|snapshot|<タグ>] [-Verify auto|require|skip] [-Force] [-NoLink]
                             [-KeepDownload <フォルダー>]
clone していなければ:
  irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1 | iex
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1))) snapshot
-Verify: auto は gh がログイン済みなら build provenance attestation を確かめる。require は確かめられなければ止める。
Windows PowerShell 5.1 と PowerShell 7 の両方で動く。iex でも使うので、失敗は exit ではなく throw で返す。
#>
param(
    [Parameter(Position = 0)][string]$Release = 'stable',
    [ValidateSet('auto', 'require', 'skip')][string]$Verify = 'auto',
    [switch]$Force,
    [switch]$NoLink,
    [string]$KeepDownload
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # 5.1 の Invoke-WebRequest は進み具合を描くと遅い
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$repo = 'fukusaka/amonhen'
$api = "https://api.github.com/repos/$repo/releases"

# 外のコマンドを動かして終了コードを返す（5.1 では標準エラーへの出力が Stop で止まるので、その間だけ Continue に）。
function Invoke-Native([scriptblock]$Command) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Command | Out-Host } finally { $ErrorActionPreference = $saved }
    $LASTEXITCODE
}

# CPU。x64 の PowerShell を ARM 機でエミュレーションしていても、OS の CPU を見る。
$arch = $null
try { $arch = [string][Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch {}
if (-not $arch) { $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE } }
$cpu = switch -regex ($arch) {
    '^(X64|AMD64)$' { 'x86_64' }
    '^(Arm64|ARM64)$' { 'aarch64' }
    default { throw "unsupported CPU: $arch" }
}

# Release を選ぶ。アプリとエンジンの正式版はタグ v<数字>（VSCode 拡張の vscode-v* とは別）。一覧は新しい順。
$headers = @{ 'User-Agent' = 'amonhen-install-engine' }
$pattern = "^amonhen-.+-windows-$cpu\.zip$"
$releases = switch ($Release) {
    'stable' { Invoke-RestMethod -Uri "$api`?per_page=100" -Headers $headers | Where-Object { $_.tag_name -match '^v\d' -and -not $_.prerelease -and -not $_.draft } }
    default { Invoke-RestMethod -Uri "$api/tags/$Release" -Headers $headers }
}
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
