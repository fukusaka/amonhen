# ログイン中の画面に試作を出して `--measure-scroll` で測り、PresentMon で出し方（PresentMode）と遅れを記録する。
# ssh のセッションには画面がないので、ログイン中の利用者のタスクとして起動する。PresentMon は管理者の権限で動かす。
#   pwsh -File measure.ps1 -Label win32 -Arguments "C:\path\to\repo --host win32 --measure-scroll"
param([string]$Arguments, [string]$Label, [int]$Seconds = 30, [string]$Exe)
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot 'measure'
New-Item -ItemType Directory -Force $dir | Out-Null
$out = "$dir\$Label.txt"; $csv = "$dir\$Label.csv"
Remove-Item -Force -ErrorAction SilentlyContinue $out, $csv
$exe = if ($Exe) { $Exe } else { Join-Path $PSScriptRoot 'target\release\amonhen-win.exe' }
$pm = (Get-Command presentmon -ErrorAction SilentlyContinue).Source
if (-not $pm) {
    $pm = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\Intel.PresentMon.Console*\presentmon.exe" | Select-Object -First 1 -ExpandProperty FullName
}
$p = Start-Process -FilePath $pm -PassThru -WindowStyle Hidden -ArgumentList @(
    '--process_name', [IO.Path]::GetFileName($exe), '--output_file', $csv, '--timed', "$Seconds",
    '--terminate_after_timed', '--stop_existing_session', '--no_console_stats')
Start-Sleep -Seconds 2
$action = New-ScheduledTaskAction -Execute $exe -Argument "$Arguments --out $out" -WorkingDirectory $PSScriptRoot
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
Register-ScheduledTask -TaskName 'AmonHenProto' -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName 'AmonHenProto'
$p.WaitForExit()
if (Test-Path $out) { Get-Content $out } else { 'no output' }
if (Test-Path $csv) {
    $rows = @(Import-Csv $csv)
    "PresentMon: $($rows.Count) presents"
    $rows | Group-Object PresentMode | ForEach-Object { "  PresentMode $($_.Name): $($_.Count)" }
    foreach ($col in 'DisplayLatency', 'MsUntilDisplayed', 'MsBetweenDisplayChange', 'MsBetweenPresents') {
        if ($rows.Count -and ($rows[0].PSObject.Properties.Name -contains $col)) {
            $v = @($rows | Where-Object { $_.$col -notin 'NA', '' } | ForEach-Object { [double]$_.$col } | Sort-Object)
            if ($v.Count) {
                '  {0}: p50 {1:N2} / p95 {2:N2} ms ({3})' -f $col, $v[[int](($v.Count - 1) * 0.5)], $v[[int](($v.Count - 1) * 0.95)], $v.Count
            }
        }
    }
} else { 'PresentMon: no CSV' }
