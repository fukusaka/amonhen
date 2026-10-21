# Windows の実機

[English](../windows.md)

Windows の PC を開発と確認に使えるように整え、Mac から操作する。エンジンと VSCode 拡張のビルドとテストは CI が
Windows でも行う（[development.md](development.md)）。実機は CI では分からないことのためにある。実際の VSCode での拡張、
複数の窓から使う共有の daemon、一瞬開くコンソールの窓、CRLF で取り出したファイル、SmartScreen、のちの Windows の FE。

実機は手元の LAN の中だけに置き、次の 3 つで操作する。

- ssh（鍵のみ）: ビルド・テスト・確認を Mac から流す（手で、または Claude が）。
- Mac の VSCode の Remote-SSH: Windows の上の clone を編集する。
- リモート デスクトップ（Mac の Windows App）: Windows の画面を見て操作する。描画がリモート デスクトップの仮想の
  ディスプレイを通るので、遅延はこれで測らない。実機の画面で測る。

## Windows で

管理者の PowerShell（スタートを右クリック →「ターミナル (管理者)」）で行う。Windows 11 Pro（Home にはリモート
デスクトップのホストがない）。

### ネットワーク

ネットワークをプライベートにし、ルーターの DHCP の予約でアドレスを固定する（アドレスが変わると ssh もリモート
デスクトップも届かなくなる）。

```powershell
Get-NetConnectionProfile
Set-NetConnectionProfile -InterfaceIndex <番号> -NetworkCategory Private
```

### 道具

```powershell
winget install --id Microsoft.PowerShell -e --installer-type wix --scope machine
winget install --id Git.Git -e
winget install --id GitHub.cli -e
winget install --id Rustlang.Rustup -e
winget install --id OpenJS.NodeJS.LTS -e
winget install --id Microsoft.VisualStudioCode -e
winget install --id Microsoft.VisualStudio.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
New-ItemProperty -Path HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem -Name LongPathsEnabled -Value 1 -PropertyType DWORD -Force
git config --global core.longpaths true
```

- PowerShell 7 は MSI 版にする（`--installer-type wix`）。Microsoft Store 版は利用者の `WindowsApps` の下に入り、ssh の
  既定のシェルにできない。`Test-Path "C:\Program Files\PowerShell\7\pwsh.exe"` で確かめる。
- `Microsoft.VisualStudio.BuildTools` は Build Tools 2026（2022 は `Microsoft.VisualStudio.2022.BuildTools`）。
  `x86_64-pc-windows-msvc` に要るのは C++ のワークロードだけ。
- Node は CI と同じ 24（`node -v`）。
- Git の既定の `core.autocrlf=true` はそのままにする。Windows の利用者の多くがこの設定なので、CRLF の作業ツリーで
  確かめられる。

### OpenSSH サーバー

`Add-WindowsCapability` は Windows Update から取ってくるので、先に待っている更新（と、求められる再起動）を済ませる。
それでも失敗するときは `winget install --id Microsoft.OpenSSH.Preview -e` で、GitHub で配っている同じ OpenSSH を
入れる（`C:\Program Files\OpenSSH` に入り、ファイアウォールの規則は入らないことがある）。

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
Set-NetFirewallRule -Name OpenSSH-Server-In-TCP -RemoteAddress LocalSubnet -Profile Private
New-ItemProperty -Path HKLM:\SOFTWARE\OpenSSH -Name DefaultShell -Value "C:\Program Files\PowerShell\7\pwsh.exe" -PropertyType String -Force
```

`DefaultShell` は、その `pwsh.exe` があることを確かめてから設定する。シェルがないと sshd は利用者を拒み（"not allowed
because shell ... does not exist"）、手元ではパスワードを聞かれるだけになる。

Mac の公開鍵を置く。Administrators の利用者の鍵は `C:\ProgramData\ssh\administrators_authorized_keys` だけが読まれ、
ASCII で書き（PowerShell 5.1 の `>` や `Out-File` は UTF-16 になる）、Administrators と SYSTEM だけが読めるようにする。
ほかの利用者は `%USERPROFILE%\.ssh\authorized_keys`。

```powershell
Set-Content -Path C:\ProgramData\ssh\administrators_authorized_keys -Value 'ssh-ed25519 AAAA... name' -Encoding ascii
icacls.exe C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant "*S-1-5-32-544:F" /grant "SYSTEM:F"
```

鍵でつながったらパスワードを止める。`C:\ProgramData\ssh\sshd_config` で `PasswordAuthentication no` と
`KbdInteractiveAuthentication no` にし、`C:\Windows\System32\OpenSSH\sshd.exe -t` で確かめてから
`Restart-Service sshd`。

拒まれるときは、sshd を前面で動かして 1 回つなぐと理由が出る。

```powershell
Stop-Service sshd
& C:\Windows\System32\OpenSSH\sshd.exe -d
```

### リモート デスクトップ

「設定 → システム → リモート デスクトップ」でオンにし（ネットワーク レベル認証はそのまま）、LAN の中だけにする。

```powershell
Get-NetFirewallRule -Name 'RemoteDesktop-*' | Set-NetFirewallRule -RemoteAddress LocalSubnet -Profile Private
```

リモート デスクトップはパスワードでサインインする。Microsoft アカウントで「Windows Hello サインインのみを許可する」に
なっていると（「設定 → アカウント → サインイン オプション」でグレーアウトしていることが多い）、パスワードが通らない。
パスワードを許し、画面をロックして（Win+L）、PIN ではなくアカウントのパスワードで 1 回サインインして覚えさせる。

```powershell
Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\PasswordLess\Device' -Name DevicePasswordLessBuildVersion -Value 0
```

Microsoft アカウント自体にパスワードが要る（account.microsoft.com の「セキュリティ → 高度なセキュリティ オプション」で
「パスワードなしのアカウント」をオフ）。値を 2 に戻すと Windows Hello のみに戻る。

### 電源と clone

```powershell
powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0
git clone https://github.com/fukusaka/amonhen.git C:\src\amonhen
```

パスは短く（`C:\src`）。Windows Update のアクティブ時間を広めにして、使っている間に再起動しないようにする。

## Mac で

`~/.ssh/config`:

```
Host winpc
  HostName <予約したアドレス>
  User <Windows の利用者名>
  IdentityFile ~/.ssh/id_windows
  IdentitiesOnly yes
```

`IdentitiesOnly` で、ほかの鍵を先に出さないようにする。すべてのホストで `ControlMaster` を使っていると、sshd を変えた
直後の確認が古い接続を使い回すことがある。`-o ControlMaster=no -o ControlPath=none` を付ける。

- VSCode: 「Remote-SSH: Connect to Host」で `winpc`、OS は Windows。`C:\src\amonhen` を開く。
- リモート デスクトップ: App Store の Windows App で、アドレス、Microsoft アカウントのメールアドレスとパスワード。

## ssh で確認を流す

引用符やパイプを含むスクリプトは、`-EncodedCommand`（UTF-16LE の base64）で送れば ssh を通っても崩れない。

```sh
ssh winpc "powershell -NoProfile -NonInteractive -EncodedCommand $(iconv -f UTF-8 -t UTF-16LE check.ps1 | base64)"
```

- `powershell -Command -`（スクリプトを標準入力から読む）は ssh 越しでは止まる。
- スクリプトの最初に `[Console]::OutputEncoding = [Text.Encoding]::UTF8` を置くと、日本語の出力を Mac で読める。
- Windows PowerShell 5.1 で `npm` は `npm.ps1` になり、テストが失敗しても `$LASTEXITCODE` が 0 のまま。`npm.cmd` を
  呼ぶ。`r` は別名（Invoke-History）なので、関数の名前にしない。
- ssh のセッションにはデスクトップがない。そこから起動した窓は画面に出ず、撮れない。画面は実機かリモート デスクトップで
  見る。

CI と同じ確認:

```powershell
cd C:\src\amonhen
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo build -p amonhen-cli
cd apps\vscode; npm.cmd ci; npm.cmd run typecheck; npm.cmd test
```

## 実際の VSCode での拡張のテスト

`npm run test:e2e`（[development.md](development.md)）は、拡張を読み込んだ VSCode を起動し、これまで手で確かめていた
ことの大半を、CRLF で取り出したファイルで確かめる。置き場所に置かれたエンジンでの起動、History、コミットの詳細と差分、
daemon と git 実行層それぞれでの行単位の stage・行の破棄・Undo・commit、拡張が起動するプロセスがすべて `windowsHide`
であること。CI も Windows で走らせるが、全体に `core.autocrlf false` を設定している。テストのリポジトリは自分で
`core.autocrlf=true` にする。

`apps\vscode` の `scripts\e2e-windows.ps1` が走らせて、終了コードを返す。`-Desktop` を付けると、ログオンしている利用者の
デスクトップで、1 回だけのタスクとして走らせ、出力を少しずつ ssh に出す。ssh のセッションにはデスクトップがないので、
ssh から VSCode が起動しないときや、VSCode を画面で（リモート デスクトップか実機で）見たいときに使う。利用者が
ログオンしている必要がある（画面のロックはかまわない）。タスクの PowerShell の窓が一瞬見えることがあるが、それは
スクリプトのもので、拡張のものではない。

```powershell
cd C:\src\amonhen
cargo build -p amonhen-cli
cd apps\vscode; npm.cmd ci
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\e2e-windows.ps1 -Desktop
```

## ブランチから作ったエンジンを VSCode で試す

拡張は、`current` のエンジンの protocol が同梱のエンジンと同じときだけそれで共有の daemon を起動し、違えば同梱の
エンジンそのもので起動する。同じ protocol の daemon が動いていれば、それを使う。そのため、protocol が新しいエンジンは、
同梱のエンジンにしたときだけ使われる。

1. ビルドする。`$env:AMONHEN_VERSION = "<入っているものより上の版>"; cargo build --release -p amonhen-cli` のあと
   `target\release\amonhen.exe install`。
2. protocol が変わったなら、拡張の
   `%USERPROFILE%\.vscode\extensions\fukusaka.amonhen-<版>\bin\amonhen-windows-x86_64.exe` に上書きする（元は残す）。
   拡張のコードも同じように試せる。`apps\vscode` で `node esbuild.mjs --production` のあと、`dist` を入っている拡張の
   `dist` に上書きする。
3. 動いている daemon を、それが動かしているエンジンで止め（`amonhen daemon stop`。版は `amonhen daemon status` で
   分かる）、VSCode で「Developer: Reload Window」。

次の snapshot かリリースの `.vsix` を入れれば、すべて元に戻る。

## 実機で確かめること

次は `npm run test:e2e` が確かめる（上の `scripts\e2e-windows.ps1` で走らせる）。

- VSCode での拡張: `%LOCALAPPDATA%\amonhen` に置かれるエンジン（`current`、`bin\amonhen.exe`）、History、コミットの詳細
  と差分、Working Copy の行単位の stage、行単位の破棄と Undo、commit。どれも CRLF で取り出したファイルで。
- `amonhen.daemon.enabled` を切って、拡張の git 実行層で同じ Working Copy の操作。
- 拡張が起動するプロセスがすべて `windowsHide`（コンソールの窓が一瞬開く原因）で、リポジトリを閉じると常駐の
  `git cat-file` が終わる。

まだ手で確かめるもの:

- 拡張を使っている間（ブランチを選ぶ、更新する）にコンソールの窓が一瞬開かない。テストが確かめるのは拡張が自分で
  起動するプロセスだけで、daemon が起動するものは含まない。
- 複数の VSCode の窓と CLI で 1 つの共有の daemon（`amonhen daemon status` でつながりが分かる）。
- Mac から実機への Remote-SSH と、そこで動く拡張。
- ブラウザーで落としたリリースの zip（SmartScreen）と、`amonhen.exe install`。
