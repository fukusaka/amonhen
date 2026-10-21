<#
Windows の実機で、Release か snapshot のエンジンを確かめる（リリースのたびに ssh で流す）。
  scripts\check-windows.ps1 [[-Release] stable|snapshot|<タグ>] [-ExpectVersion <版>] [-SkipInstall]
                            [-Verify auto|require|skip] [-KeepTemp]
install-engine.ps1 で入れ（current もこの版に向ける）、置いたファイル、CRLF のリポジトリ（core.autocrlf=true）での
CLI の log・status・diff・stage・discard・commit・undo、共有の daemon を 2 つのクライアントで使えることを確かめ、
結果を表にする。1 つでも通らなければ終了コードは 1。
#>
param(
    [Parameter(Position = 0)][string]$Release = 'stable',
    [string]$ExpectVersion,
    [switch]$SkipInstall,
    [ValidateSet('auto', 'require', 'skip')][string]$Verify = 'auto',
    [switch]$KeepTemp
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$OutputEncoding = [Text.Encoding]::UTF8

$results = New-Object System.Collections.ArrayList
function Add-Result([string]$Name, [string]$Result, [string]$Detail) {
    [void]$results.Add([pscustomobject]@{ Check = $Name; Result = $Result; Detail = $Detail })
    $color = @{ PASS = 'Green'; FAIL = 'Red'; SKIP = 'Yellow' }[$Result]
    Write-Host ("[{0}] {1}: {2}" -f $Result, $Name, $Detail) -ForegroundColor $color
}
# 確かめる 1 項目。$Body が文字列を返せば PASS（その説明）、throw すれば FAIL。
function Test-Step([string]$Name, [scriptblock]$Body) {
    try { Add-Result $Name 'PASS' ([string](& $Body)) } catch { Add-Result $Name 'FAIL' $_.Exception.Message }
}
# 外のコマンドの標準出力を文字列で返す（標準エラーは捨てる）。終了コードが 0 でなければ throw。
function Invoke-Tool([string]$Exe, [string[]]$Arguments) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Exe @Arguments 2>$null } finally { $ErrorActionPreference = $saved }
    if ($LASTEXITCODE -ne 0) { throw "$(Split-Path -Leaf $Exe) $($Arguments -join ' ') exited with $LASTEXITCODE" }
    ($out | Out-String).TrimEnd()
}
function Invoke-Git([string[]]$Arguments) { Invoke-Tool 'git' (@('-C', $repo) + $Arguments) }
function Invoke-Amonhen([string[]]$Arguments) { Invoke-Tool $amonhen $Arguments }
function Get-Sha1Prefix([string]$Path) {
    $sha = [Security.Cryptography.SHA1]::Create()
    try { (-join ($sha.ComputeHash([IO.File]::ReadAllBytes($Path)) | ForEach-Object { $_.ToString('x2') })).Substring(0, 16) } finally { $sha.Dispose() }
}
function Get-Text([string]$Path) { [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($Path)) }
function Show-Bytes([string]$Text) { $Text.Replace("`r", '\r').Replace("`n", '\n') }
# diff の JSON（ファイルの配列）から、f.txt の行のうち種類と中身が合うものの番号（配列の位置が行の番号）。
function Find-Row($Diff, [string]$Kind, [string]$Text) {
    $rows = @(@($Diff) | Where-Object { $_.path -eq 'f.txt' })[0].rows
    for ($i = 0; $i -lt $rows.Count; $i++) { if ($rows[$i].kind -eq $Kind -and $rows[$i].text -eq $Text) { return $i } }
    throw "no $Kind row '$Text' in the diff"
}

$root = Join-Path $env:LOCALAPPDATA 'amonhen'
$amonhen = Join-Path $root 'bin\amonhen.exe'
$temp = Join-Path ([IO.Path]::GetTempPath()) "amonhen-check-$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
New-Item -ItemType Directory -Path $temp | Out-Null
$downloads = Join-Path $temp 'download'

# 1. 入れる
$zip = $null
if ($SkipInstall) {
    Add-Result 'install' 'SKIP' 'using the installed engine'
} else {
    $installer = Join-Path $PSScriptRoot 'install-engine.ps1'
    if (-not $PSScriptRoot -or -not (Test-Path $installer)) {
        $installer = Join-Path $temp 'install-engine.ps1'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://raw.githubusercontent.com/fukusaka/amonhen/main/scripts/install-engine.ps1' -OutFile $installer
    }
    $log = $null
    try {
        $log = & $installer $Release -Verify $Verify -Force -KeepDownload $downloads 6>&1 | Out-String
        $zip = Get-ChildItem -Path $downloads -Filter 'amonhen-*.zip' | Select-Object -First 1
        Add-Result 'install' 'PASS' $zip.Name
    } catch {
        Add-Result 'install' 'FAIL' $_.Exception.Message
    }
    Write-Host $log
    $attest = [regex]::Match([string]$log, 'attestation: (.*)').Groups[1].Value.Trim()
    if ($attest -eq 'verified') { Add-Result 'attestation' 'PASS' 'gh attestation verify' }
    elseif ($attest -match 'failed') { Add-Result 'attestation' 'FAIL' $attest }
    else { Add-Result 'attestation' 'SKIP' $(if ($attest) { $attest } else { 'not run' }) }
}
if (-not $ExpectVersion -and $zip) { $ExpectVersion = [regex]::Match($zip.Name, '^amonhen-(.+)-windows-[^-]+\.zip$').Groups[1].Value }

# 2. Mark of the Web（Invoke-WebRequest で取ったものには付かないはず）
if ($zip) {
    Test-Step 'mark of the web' {
        $marked = @($zip.FullName, $amonhen) | Where-Object { Get-Item -Path $_ -Stream Zone.Identifier -ErrorAction SilentlyContinue }
        if ($marked) { throw "Zone.Identifier on $($marked -join ', ')" }
        'no Zone.Identifier on the zip and bin\amonhen.exe'
    }
}

# 3. 版
Test-Step 'version' {
    $out = Invoke-Amonhen @('--version')
    if ($ExpectVersion -and $out -notlike "amonhen $ExpectVersion (protocol *") { throw "$out (expected $ExpectVersion)" }
    $out
}

# 4. 置いたファイル: current → engine\<ハッシュ>\amonhen.exe（ハッシュは中身の SHA-1）、bin\amonhen.exe は同じもの
Test-Step 'files' {
    $current = (Get-Content -Raw (Join-Path $root 'current')).Trim()
    $engine = Join-Path (Join-Path $root $current) 'amonhen.exe'
    if (-not (Test-Path $engine)) { throw "current ($current) has no amonhen.exe" }
    $hash = Get-Sha1Prefix $engine
    if ((Split-Path -Leaf $current) -ne $hash) { throw "current is $current but its amonhen.exe hashes to $hash" }
    if (-not (Test-Path $amonhen)) { throw 'bin\amonhen.exe is missing' }
    if ((Get-Sha1Prefix $amonhen) -ne $hash) { throw 'bin\amonhen.exe differs from the current engine' }
    "current = $current, bin\amonhen.exe is the same"
}

# 5. CRLF のリポジトリ: f.txt（a〜e の 5 行）を LF でコミットし、core.autocrlf=true で CRLF で取り出す
$repo = Join-Path $temp 'repo'
$crlf = "a`r`nB`r`nc`r`nD`r`ne`r`n"
try {
    New-Item -ItemType Directory -Path $repo | Out-Null
    Invoke-Git @('init', '-q', '-b', 'main') | Out-Null
    Invoke-Git @('config', 'core.autocrlf', 'true') | Out-Null
    Invoke-Git @('config', 'core.safecrlf', 'false') | Out-Null
    Invoke-Git @('config', 'user.name', 'Amon Hen check') | Out-Null
    Invoke-Git @('config', 'user.email', 'check@example.invalid') | Out-Null
    [IO.File]::WriteAllText((Join-Path $repo 'f.txt'), "a`nb`nc`nd`ne`n")
    Invoke-Git @('add', 'f.txt') | Out-Null
    Invoke-Git @('commit', '-q', '-m', 'one') | Out-Null
    [IO.File]::WriteAllText((Join-Path $repo 'g.txt'), "x`n")
    Invoke-Git @('add', 'g.txt') | Out-Null
    Invoke-Git @('commit', '-q', '-m', 'two') | Out-Null
    Remove-Item (Join-Path $repo 'f.txt')
    Invoke-Git @('checkout', '--', 'f.txt') | Out-Null
    $checkedOut = Get-Text (Join-Path $repo 'f.txt')
    if ($checkedOut -ne "a`r`nb`r`nc`r`nd`r`ne`r`n") { throw "f.txt was checked out as $(Show-Bytes $checkedOut)" }
    # b と d を変える（CRLF のまま）
    [IO.File]::WriteAllText((Join-Path $repo 'f.txt'), $crlf)
    $repoReady = $true
} catch {
    Add-Result 'repository' 'FAIL' $_.Exception.Message
    $repoReady = $false
}

if ($repoReady) {
    Test-Step 'log' {
        $rows = Invoke-Amonhen @('log', '--json', '-C', $repo) | ConvertFrom-Json
        $rows = @($rows) # 5.1 の ConvertFrom-Json は配列を 1 つのものとして流すので、変数に入れてから配列に
        if ($rows.Count -ne 2 -or $rows[0].subject -ne 'two') { throw "$($rows.Count) commits, first '$($rows[0].subject)'" }
        '2 commits'
    }
    Test-Step 'status' {
        $s = Invoke-Amonhen @('status', '--json', '-C', $repo) | ConvertFrom-Json
        $e = @($s.entries | Where-Object { $_.path -eq 'f.txt' })
        if ($e.Count -ne 1 -or -not $e[0].unstaged -or $e[0].staged) { throw "f.txt: $($e | ConvertTo-Json -Compress)" }
        if (@($s.entries).Count -ne 1) { throw "$(@($s.entries).Count) changed files (expected 1)" }
        'f.txt unstaged'
    }
    Test-Step 'diff' {
        $diff = Invoke-Amonhen @('diff', '--json', '-C', $repo) | ConvertFrom-Json
        $f = @($diff) | Where-Object { $_.path -eq 'f.txt' }
        # 改行の違いで全行が変わったことにならず、変えた 2 行だけが 1 行ずつ出る
        if ($f.added -ne 2 -or $f.removed -ne 2) { throw "+$($f.added) -$($f.removed) (expected +2 -2)" }
        $le = $f.lineEndings
        if (-not $le -or $le.worktree -ne 'crlf' -or $le.index -ne 'lf' -or -not $le.notable) { throw "lineEndings: $($le | ConvertTo-Json -Compress)" }
        "+2 -2, lineEndings worktree $($le.worktree), index $($le.index), $($le.rule)"
    }
    Test-Step 'stage line' {
        $row = Find-Row (Invoke-Amonhen @('diff', '--combined', '--json', '-C', $repo) | ConvertFrom-Json) 'added' 'B'
        Invoke-Amonhen @('stage', '--rows', "$row", '-C', $repo, 'f.txt') | Out-Null
        $eol = Invoke-Git @('ls-files', '--eol', '--', 'f.txt')
        if ($eol -notmatch '^i/lf\s') { throw "index: $eol" }
        $index = (Invoke-Git @('show', ':f.txt')) -split "`r?`n"
        if (($index -join ',') -ne 'a,B,c,d,e') { throw "index has $($index -join ',')" }
        "row $row staged; index LF only (a,B,c,d,e)"
    }
    Test-Step 'discard lines' {
        $diff = Invoke-Amonhen @('diff', '--json', '-C', $repo) | ConvertFrom-Json
        $rows = "$(Find-Row $diff 'removed' 'd'),$(Find-Row $diff 'added' 'D')"
        Invoke-Amonhen @('discard', '--rows', $rows, '-C', $repo, 'f.txt') | Out-Null
        $now = Get-Text (Join-Path $repo 'f.txt')
        if ($now -ne "a`r`nB`r`nc`r`nd`r`ne`r`n") { throw "f.txt is $(Show-Bytes $now)" }
        "rows $rows discarded; other lines keep CRLF"
    }
    $before = $null
    Test-Step 'commit' {
        $script:before = Invoke-Git @('rev-parse', 'HEAD')
        Invoke-Amonhen @('commit', '-m', 'three', '-C', $repo) | Out-Null
        $subject = Invoke-Git @('log', '-1', '--format=%s')
        if ($subject -ne 'three') { throw "HEAD is '$subject'" }
        $committed = (Invoke-Git @('show', 'HEAD:f.txt')) -split "`r?`n"
        if (($committed -join ',') -ne 'a,B,c,d,e') { throw "committed f.txt has $($committed -join ',')" }
        $eol = Invoke-Git @('ls-files', '--eol', '--', 'f.txt')
        if ($eol -notmatch '^i/lf\s') { throw "index: $eol" }
        "HEAD $(Invoke-Git @('rev-parse', '--short', 'HEAD')) 'three'"
    }
    Test-Step 'undo' {
        if (-not $before) { throw 'nothing to undo (commit failed)' }
        Invoke-Amonhen @('undo', '-C', $repo) | Out-Null
        $head = Invoke-Git @('rev-parse', 'HEAD')
        if ($head -ne $before) { throw "HEAD is $head (expected $before)" }
        'HEAD back to the commit before'
    }

    # 6. 共有の daemon: daemon: のクライアントを 2 つ常駐させ（watch）、同じ daemon が 2 つとも受けていること
    $watchers = @()
    $wasRunning = $false
    Test-Step 'shared daemon' {
        $saved = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        & $amonhen daemon status --json 2>$null | Out-Null
        $script:wasRunning = $LASTEXITCODE -eq 0
        $ErrorActionPreference = $saved
        for ($i = 1; $i -le 2; $i++) {
            $script:watchers += Start-Process -FilePath $amonhen -ArgumentList @('watch', '--json', '-C', "`"daemon:$repo`"") -NoNewWindow -PassThru `
                -RedirectStandardOutput (Join-Path $temp "watch$i.out") -RedirectStandardError (Join-Path $temp "watch$i.err")
        }
        $leaf = Split-Path -Leaf $temp
        $entry = $null
        for ($t = 0; $t -lt 40 -and -not ($entry -and $entry.clients -ge 2); $t++) {
            Start-Sleep -Milliseconds 250
            $exited = @($script:watchers | Where-Object { $_.HasExited })
            if ($exited) { throw "a watch client exited: $(Get-Content -Raw (Join-Path $temp 'watch1.err'))$(Get-Content -Raw (Join-Path $temp 'watch2.err'))" }
            try { $status = Invoke-Amonhen @('daemon', 'status', '--json') | ConvertFrom-Json } catch { continue }
            $entry = @($status.repos | Where-Object { $_.path -like "*$leaf*" })[0]
        }
        if (-not $entry) { throw 'the daemon does not list the repository' }
        if ($entry.clients -lt 2) { throw "$($entry.clients) client(s) on the repository" }
        $note = if ($script:wasRunning) { 'already running' } else { 'started by the clients' }
        if (-not $script:wasRunning -and $ExpectVersion -and $status.version -ne $ExpectVersion) { throw "daemon is $($status.version) (expected $ExpectVersion)" }
        "pid $($status.pid), $($status.version), $($entry.clients) clients ($note)"
    }
    $watchers | Where-Object { -not $_.HasExited } | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
    if (-not $wasRunning) {
        $ErrorActionPreference = 'Continue'
        & $amonhen daemon stop 2>$null | Out-Null
        $ErrorActionPreference = 'Stop'
    }
}

if ($KeepTemp) { Write-Host "kept $temp" } else { Start-Sleep -Milliseconds 500; Remove-Item -Recurse -Force -Path $temp -ErrorAction SilentlyContinue }

Write-Host ''
$width = ($results | ForEach-Object { $_.Check.Length } | Measure-Object -Maximum).Maximum
foreach ($r in $results) { Write-Host ("{0}  {1}  {2}" -f $r.Check.PadRight($width), $r.Result, $r.Detail) }
$failed = @($results | Where-Object { $_.Result -eq 'FAIL' }).Count
Write-Host ''
if ($failed) { Write-Host "$failed check(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'all checks passed' -ForegroundColor Green
exit 0
