# VSCode を起動する結合テスト（npm run test:e2e）を Windows で走らせ、出力を出して終了コードを返す。
# -Desktop を付けると、ログオン中の利用者のデスクトップで走らせる（1 回だけのタスクをタスク スケジューラに登録して
# 待つ）。ssh のセッションには画面がないので、ssh から実機の画面の上で VSCode を動かすときに使う。
# 先にルートで cargo build -p amonhen-cli、apps\vscode で npm.cmd ci をしておく。
# Windows PowerShell 5.1 でも日本語を読めるように、BOM 付きの UTF-8 で保存する
param(
  [switch]$Desktop,
  # タスクの中から呼ばれるときの、出力と終了コードを書く場所（内部用）
  [string]$Out
)
$ErrorActionPreference = 'Stop'
$vscode = Split-Path -Parent $PSScriptRoot

if ($Out) {
  # npm と VSCode の出力は UTF-8
  [Console]::OutputEncoding = [Text.Encoding]::UTF8
  Set-Location $vscode
  & npm.cmd run test:e2e *> (Join-Path $Out 'log.txt')
  Set-Content -Path (Join-Path $Out 'exit.txt') -Value $LASTEXITCODE -Encoding ascii
  exit
}
if (-not $Desktop) {
  Set-Location $vscode
  & npm.cmd run test:e2e
  exit $LASTEXITCODE
}

# ログオン中でなければ、タスクは走らない
$user = "$env:USERDOMAIN\$env:USERNAME"
if (-not (Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue | Where-Object UserName -eq $user)) {
  Write-Error "$user がデスクトップにログオンしていません（画面のロックはかまわない）"
}
$out = Join-Path $env:TEMP "amonhen-e2e-$PID"
New-Item -ItemType Directory -Force $out | Out-Null
$name = "amonhen-e2e-$PID"
$shell = (Get-Process -Id $PID).Path
$action = New-ScheduledTaskAction -Execute $shell -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$PSCommandPath`" -Out `"$out`""
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $name -Action $action -Principal $principal | Out-Null
$code = 1
try {
  Start-ScheduledTask -TaskName $name
  $log = Join-Path $out 'log.txt'
  $exit = Join-Path $out 'exit.txt'
  $shown = 0
  $start = Get-Date
  # 出力を少しずつ出しながら、終わるまで待つ
  while (-not (Test-Path $exit)) {
    Start-Sleep -Seconds 2
    if (Test-Path $log) {
      $lines = @(Get-Content $log)
      $lines | Select-Object -Skip $shown | Write-Host
      $shown = $lines.Count
    } elseif (((Get-Date) - $start).TotalSeconds -gt 60) {
      Write-Error 'タスクが始まりません'
    }
    if (((Get-Date) - $start).TotalMinutes -gt 20) {
      Write-Error '20 分たっても終わりません'
    }
  }
  @(Get-Content $log) | Select-Object -Skip $shown | Write-Host
  $code = [int](Get-Content $exit)
} finally {
  Unregister-ScheduledTask -TaskName $name -Confirm:$false
  Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
}
exit $code
