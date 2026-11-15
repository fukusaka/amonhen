# ログイン中の画面を撮る（ssh から、利用者のタスクとして起動する）。
#   pwsh -File screenshot.ps1 -Path C:\...\shot.png
param([string]$Path, [switch]$Run)
if ($Run) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    # 前にいるウィンドウと、試作の窓の状態も残す。
    Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class W {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
}
'@
    $s = New-Object System.Text.StringBuilder 256
    [void][W]::GetWindowText([W]::GetForegroundWindow(), $s, 256)
    "foreground: $s" | Out-File "$Path.txt"
    return
}
Remove-Item -Force -ErrorAction SilentlyContinue $Path, "$Path.txt"
$action = New-ScheduledTaskAction -Execute 'C:\Program Files\PowerShell\7\pwsh.exe' -Argument "-NoProfile -WindowStyle Hidden -File `"$PSCommandPath`" -Run -Path `"$Path`""
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
Register-ScheduledTask -TaskName 'AmonHenShot' -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName 'AmonHenShot'
for ($i = 0; $i -lt 40 -and -not (Test-Path "$Path.txt"); $i++) { Start-Sleep -Milliseconds 250 }
Get-Content "$Path.txt" -ErrorAction SilentlyContinue
