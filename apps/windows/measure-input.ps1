# 入力から画面までを測る。試作を `--measure-input <n>` で起動し、ログイン中の画面で窓の History にクリックを
# n 回送る（`SendInput` の直前の QPC を記録）。試作は受け取った時刻と、それを映したフレームが出た時刻を書く。
# 同じ機械の QPC なので、送ってから画面までをそのまま引ける。PresentMon の click-to-photon も併せて読む。
#   pwsh -File measure-input.ps1 -Label win32 -Repo C:\path\to\repo -HostKind win32
#   -Manual: 送らずに、画面の前の人が窓の History（右側）を -Count 回クリックする。基準は別プロセスの Raw Input
#   （`--listen`）で、OS が入力を受けてからアプリに届くまで・画面に出るまでを出す。
param([string]$Label, [string]$Repo, [string]$HostKind = 'win32', [string]$Extra = '', [int]$Count = 100, [string]$Title, [switch]$Send, [string]$SendsPath, [switch]$Manual, [int]$Wait = 180, [string]$Exe)

$code = @'
using System; using System.Runtime.InteropServices; using System.Diagnostics; using System.Threading;
public static class Clicker {
  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string title);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  static INPUT M(uint flags) { var i = new INPUT(); i.type = 0; i.mi.dwFlags = flags; return i; }
  public static long[] Run(string title, int count) {
    IntPtr h = IntPtr.Zero;
    for (int i = 0; i < 300 && h == IntPtr.Zero; i++) { h = FindWindow(null, title); Thread.Sleep(100); }
    if (h == IntPtr.Zero) throw new Exception("window not found: " + title);
    Thread.Sleep(2000);
    // 送る側の pwsh も Windows Terminal の窓を開くことがあり、クリックする所を覆うので、試作の窓を前に出す。
    SetForegroundWindow(h);
    Thread.Sleep(500);
    RECT r; GetWindowRect(h, out r);
    SetCursorPos(r.Left + (r.Right - r.Left) * 7 / 10, r.Top + (r.Bottom - r.Top) / 2);
    var rnd = new Random(1);
    var sends = new long[count];
    for (int i = 0; i < count; i++) {
      Thread.Sleep(150 + rnd.Next(100));
      sends[i] = Stopwatch.GetTimestamp();
      SendInput(2, new[] { M(0x0002), M(0x0004) }, Marshal.SizeOf(typeof(INPUT)));
    }
    return sends;
  }
}
'@

if ($Send) {
    # ログイン中の画面で動く側。
    Add-Type -TypeDefinition $code
    [Clicker]::Run($Title, $Count) | Out-File $SendsPath
    return
}

$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot 'measure'
New-Item -ItemType Directory -Force $dir | Out-Null
$out = "$dir\$Label.txt"; $csv = "$dir\$Label.csv"; $sends = "$dir\$Label.sends.txt"; $os = "$dir\$Label.os.txt"
Remove-Item -Force -ErrorAction SilentlyContinue $out, $csv, $sends, $os
# -HostKind gpui は別の実行ファイル（`gpui/`）。画面に出た時刻は PresentMon（`--qpc_time`）から求める。
$title = switch ($HostKind) { 'winui' { 'Amon Hen (WinUI 3)' } 'gpui' { 'Amon Hen (GPUI)' } default { 'Amon Hen (Win32)' } }
$exe = if ($Exe) { $Exe } elseif ($HostKind -eq 'gpui') { Join-Path $PSScriptRoot 'gpui\target\release\amonhen-win-gpui.exe' } else { Join-Path $PSScriptRoot 'target\release\amonhen-win.exe' }
$procName = [IO.Path]::GetFileName($exe)
$pm = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\Intel.PresentMon.Console*\presentmon.exe" | Select-Object -First 1 -ExpandProperty FullName
$p = Start-Process -FilePath $pm -PassThru -WindowStyle Hidden -ArgumentList @(
    '--process_name', $procName, '--output_file', $csv, '--timed', $(if ($Manual) { $Wait + 30 } else { 60 }),
    '--terminate_after_timed', '--stop_existing_session', '--no_console_stats', '--v2_metrics', '--qpc_time')
Start-Sleep -Seconds 2
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
$app = New-ScheduledTaskAction -Execute $exe -Argument "$Repo $(if ($HostKind -ne 'gpui') { "--host $HostKind" }) --measure-input $Count $Extra --out $out" -WorkingDirectory $PSScriptRoot
Register-ScheduledTask -TaskName 'AmonHenProto' -Action $app -Principal $principal -Force | Out-Null
$listen = New-ScheduledTaskAction -Execute (Join-Path $PSScriptRoot 'target\release\amonhen-win.exe') -Argument "--listen `"$os`"" -WorkingDirectory $PSScriptRoot
Register-ScheduledTask -TaskName 'AmonHenListen' -Action $listen -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName 'AmonHenListen'
$sender = New-ScheduledTaskAction -Execute 'C:\Program Files\PowerShell\7\pwsh.exe' -Argument "-NoProfile -WindowStyle Hidden -File `"$PSCommandPath`" -Send -Title `"$title`" -Count $Count -SendsPath `"$sends`""
Register-ScheduledTask -TaskName 'AmonHenClick' -Action $sender -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName 'AmonHenProto'
if ($Manual) {
    # アプリは -Count 回分を測り終えると自分で閉じる。
    Start-Sleep -Seconds 3
    for ($i = 0; $i -lt $Wait -and -not (Select-String -Quiet -Path $out -Pattern 'input received to display:' -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Seconds 1 }
    Start-Sleep -Seconds 1
} else {
    Start-ScheduledTask -TaskName 'AmonHenClick'
    for ($i = 0; $i -lt 120 -and -not (Test-Path $sends); $i++) { Start-Sleep -Seconds 1 }
    Start-Sleep -Seconds 2
}
Get-Process amonhen-win, amonhen-win-gpui -ErrorAction SilentlyContinue | Stop-Process -Force
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue

$freq = [System.Diagnostics.Stopwatch]::Frequency
$lines = Get-Content $out
$lines | Where-Object { $_ -notmatch '^input ' }
$sent = @(if (Test-Path $sends) { Get-Content $sends | ForEach-Object { [long]$_ } })
$seen = @(if (Test-Path $os) { Get-Content $os | ForEach-Object { [long]$_ } })
# PresentMon の記録から、present ごとの（present を呼んだ QPC、画面に出た QPC）。v2 の CPUStartQPC は前の
# present が戻った時刻なので、呼んだ時刻は CPUStartQPC + CPUBusy、出た時刻は CPUStartQPC + DisplayLatency。
$presents = @(if (Test-Path $csv) {
    Import-Csv $csv | Where-Object { $_.DisplayLatency -notin 'NA', '' } | ForEach-Object {
        $startCol = if ($_.PSObject.Properties.Name -contains 'CPUStartQPC') { 'CPUStartQPC' } else { 'CPUStartTime' }
        $c = [long]$_.$startCol
        [pscustomobject]@{ start = $c + [long]([double]$_.CPUBusy * $freq / 1000.0); shown = $c + [long]([double]$_.DisplayLatency * $freq / 1000.0) }
    } | Sort-Object start
})
$pmGaps = @()
$pairs = foreach ($l in $lines) {
    if ($l -match '^input received=(\d+)(?: displayed=(\d+))?') {
        $r = [long]$Matches[1]
        # 受け取った後に呼んだ最初の present が、その入力を映す。
        $pm = $presents | Where-Object { $_.start -ge $r } | Select-Object -First 1
        $d = if ($Matches[2]) { [long]$Matches[2] } elseif ($pm) { $pm.shown } else { $null }
        if ($Matches[2] -and $pm) { $pmGaps += ($pm.shown - [long]$Matches[2]) * 1000.0 / $freq }
        if (-not $d) { continue }
        $s = ($sent | Where-Object { $_ -le $r } | Measure-Object -Maximum).Maximum
        $o = ($seen | Where-Object { $_ -le $r } | Measure-Object -Maximum).Maximum
        [pscustomobject]@{
            deliver = if ($s) { ($r - $s) * 1000.0 / $freq }
            total = if ($s) { ($d - $s) * 1000.0 / $freq }
            sendToOs = if ($s -and $o -and $o -ge $s) { ($o - $s) * 1000.0 / $freq }
            osDeliver = if ($o) { ($r - $o) * 1000.0 / $freq }
            osTotal = if ($o) { ($d - $o) * 1000.0 / $freq }
        }
    }
}
Get-ScheduledTask -TaskName 'AmonHenListen' | Stop-ScheduledTask
function Show($name, $values) {
    $v = @($values | Sort-Object)
    if ($v.Count) { '{0}: p50 {1:N2} / p95 {2:N2} / max {3:N2} ms ({4})' -f $name, $v[[int](($v.Count - 1) * 0.5)], $v[[int](($v.Count - 1) * 0.95)], $v[-1], $v.Count }
}
"sent $($sent.Count) clicks, other process saw $($seen.Count)"
Show 'SendInput to received' ($pairs | Where-Object deliver -ne $null | ForEach-Object deliver)
Show 'SendInput to display' ($pairs | Where-Object total -ne $null | ForEach-Object total)
Show 'SendInput to other process raw input' ($pairs | Where-Object sendToOs -ne $null | ForEach-Object sendToOs)
Show 'other process raw input to received' ($pairs | Where-Object osDeliver -ne $null | ForEach-Object osDeliver)
Show 'other process raw input to display' ($pairs | Where-Object osTotal -ne $null | ForEach-Object osTotal)
Show 'PresentMon display minus own display (check)' $pmGaps
if (Test-Path $csv) {
    $rows = @(Import-Csv $csv)
    $rows | Group-Object PresentMode | ForEach-Object { "  PresentMode $($_.Name): $($_.Count)" }
    foreach ($col in $rows[0].PSObject.Properties.Name | Where-Object { $_ -match 'Photon|Input' }) {
        Show "  PresentMon $col" ($rows | Where-Object { $_.$col -notin 'NA', '' } | ForEach-Object { [double]$_.$col })
    }
} else { 'PresentMon: no CSV' }
