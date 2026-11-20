# CI の Windows の runner で、試験と CPU を取り合う OS の仕事を止める（使い捨ての VM なので戻さない）。
# runner は起動して数分のうちに Windows Update（wuauserv・UsoSvc と、更新を当てる TrustedInstaller の TiWorker）と、
# それに続く Storage Service（StorSvc）が 1 コア以上を数十秒使う。VSCode 拡張の e2e で測ると、重なった間は git の
# commit が 1 回 1.5〜5 秒（ふだんは 0.15 秒）かかり、拡張ホストが止まって試験が時間切れになった。
# 止められないもの（ないもの・守られているもの）は、そう言って先へ進む。
$ErrorActionPreference = 'Continue'
foreach ($name in 'wuauserv', 'UsoSvc', 'TrustedInstaller', 'StorSvc') {
    $service = Get-Service -Name $name -ErrorAction SilentlyContinue
    if (-not $service) {
        "${name}: not installed"
        continue
    }
    $was = $service.Status
    try {
        Set-Service -Name $name -StartupType Disabled -ErrorAction Stop
        Stop-Service -Name $name -Force -ErrorAction Stop
        "${name}: $was -> $((Get-Service -Name $name).Status), disabled"
    } catch {
        "${name}: $was, can't stop: $($_.Exception.Message)"
    }
}
exit 0
