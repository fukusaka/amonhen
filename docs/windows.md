# Windows machine

[日本語](ja/windows.md)

Setting up a Windows PC for development and checks, and driving it from a Mac. CI builds and tests the engines and the
VSCode extension on Windows (see [development.md](development.md)); a real machine is for what CI can't show: the
extension in a real VSCode, the shared daemon used from several windows, console windows that flash, files checked out
with CRLF, SmartScreen, and later a Windows FE.

The machine stays on the local network. It is driven three ways:

- ssh, with a key only: builds, tests and checks, run from the Mac (by hand or by Claude).
- Remote-SSH from VSCode on the Mac: editing a clone on the Windows machine.
- Remote Desktop (the Windows App on the Mac): looking at and using the Windows screen. Don't measure latency through
  it: drawing goes through Remote Desktop's virtual display. Measure on the machine's own screen.

## On Windows

Run these in PowerShell as an administrator (right-click Start, Terminal (Admin)), on Windows 11 Pro (Home has no
Remote Desktop host).

### Network

Make the network private, and give the machine a fixed address with a DHCP reservation on the router (ssh and Remote
Desktop stop reaching it when the address changes).

```powershell
Get-NetConnectionProfile
Set-NetConnectionProfile -InterfaceIndex <index> -NetworkCategory Private
```

### Tools

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

- PowerShell 7 has to be the MSI (`--installer-type wix`): the Microsoft Store version lives under the user's
  `WindowsApps` and can't be ssh's default shell. Check with `Test-Path "C:\Program Files\PowerShell\7\pwsh.exe"`.
- `Microsoft.VisualStudio.BuildTools` is Build Tools 2026 (`Microsoft.VisualStudio.2022.BuildTools` is 2022). Its C++
  workload is all `x86_64-pc-windows-msvc` needs.
- Node should be 24, like CI (`node -v`).
- Keep Git's default `core.autocrlf=true`: most Windows users have it, so checks run against CRLF working trees.

### OpenSSH server

`Add-WindowsCapability` downloads from Windows Update, so finish pending updates (and the restart they ask for) first.
If it still fails, `winget install --id Microsoft.OpenSSH.Preview -e` installs the same OpenSSH from GitHub (into
`C:\Program Files\OpenSSH`, possibly without the firewall rule).

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
Set-NetFirewallRule -Name OpenSSH-Server-In-TCP -RemoteAddress LocalSubnet -Profile Private
New-ItemProperty -Path HKLM:\SOFTWARE\OpenSSH -Name DefaultShell -Value "C:\Program Files\PowerShell\7\pwsh.exe" -PropertyType String -Force
```

Set `DefaultShell` only once that `pwsh.exe` exists: with a missing shell, sshd refuses the user ("not allowed because
shell ... does not exist") and the client just falls back to asking for a password.

Put the Mac's public key in place. For a user in Administrators, sshd reads only
`C:\ProgramData\ssh\administrators_authorized_keys`, which must be ASCII (PowerShell 5.1's `>` and `Out-File` write
UTF-16) and readable only by Administrators and SYSTEM. For other users it is `%USERPROFILE%\.ssh\authorized_keys`.

```powershell
Set-Content -Path C:\ProgramData\ssh\administrators_authorized_keys -Value 'ssh-ed25519 AAAA... name' -Encoding ascii
icacls.exe C:\ProgramData\ssh\administrators_authorized_keys /inheritance:r /grant "*S-1-5-32-544:F" /grant "SYSTEM:F"
```

Once the key works, turn off passwords: in `C:\ProgramData\ssh\sshd_config`, set `PasswordAuthentication no` and
`KbdInteractiveAuthentication no`, check with `C:\Windows\System32\OpenSSH\sshd.exe -t`, then `Restart-Service sshd`.

When something is refused, run sshd in the foreground and connect once; it prints the reason:

```powershell
Stop-Service sshd
& C:\Windows\System32\OpenSSH\sshd.exe -d
```

### Remote Desktop

Turn it on in Settings, System, Remote Desktop (keep Network Level Authentication), and limit it to the local network:

```powershell
Get-NetFirewallRule -Name 'RemoteDesktop-*' | Set-NetFirewallRule -RemoteAddress LocalSubnet -Profile Private
```

Remote Desktop signs in with a password. With a Microsoft account set to "only allow Windows Hello sign-in" (often
greyed out in Settings, Accounts, Sign-in options), the password doesn't work there. Allow passwords, then lock the
screen (Win+L) and sign in once with the account's password (not the PIN) so the machine knows it:

```powershell
Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\PasswordLess\Device' -Name DevicePasswordLessBuildVersion -Value 0
```

The Microsoft account itself must have a password (account.microsoft.com, Security, Advanced security options,
"Passwordless account" off). Setting the value back to 2 restores Windows Hello only.

### Power and the clone

```powershell
powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0
git clone https://github.com/fukusaka/amonhen.git C:\src\amonhen
```

Keep the path short (`C:\src`). Set Windows Update's active hours wide enough that it doesn't restart while in use.

## On the Mac

`~/.ssh/config`:

```
Host winpc
  HostName <the reserved address>
  User <the Windows user name>
  IdentityFile ~/.ssh/id_windows
  IdentitiesOnly yes
```

`IdentitiesOnly` keeps ssh from offering other keys first. If `ControlMaster` is on for all hosts, a check right after
changing sshd may reuse an old connection: add `-o ControlMaster=no -o ControlPath=none`.

- VSCode: Remote-SSH: Connect to Host, `winpc`, platform Windows. Open `C:\src\amonhen`.
- Remote Desktop: the Windows App from the App Store, with the address, the Microsoft account's email address and its
  password.

## Running checks over ssh

A script with quotes and pipes survives ssh unchanged when it is sent as `-EncodedCommand` (base64 of UTF-16LE):

```sh
ssh winpc "powershell -NoProfile -NonInteractive -EncodedCommand $(iconv -f UTF-8 -t UTF-16LE check.ps1 | base64)"
```

- `powershell -Command -` (reading the script from standard input) hangs over ssh.
- Start a script with `[Console]::OutputEncoding = [Text.Encoding]::UTF8` to read Japanese output on the Mac.
- In Windows PowerShell 5.1, `npm` runs `npm.ps1`, which leaves `$LASTEXITCODE` at 0 even when tests fail. Call
  `npm.cmd`. `r` is an alias (Invoke-History), so don't name a function `r`.
- An ssh session has no desktop: windows started from it don't appear on the screen and can't be captured. Look at the
  screen in person or through Remote Desktop.

The same checks as CI:

```powershell
cd C:\src\amonhen
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
cargo build -p amonhen-cli
cd apps\vscode; npm.cmd ci; npm.cmd run typecheck; npm.cmd test
```

## The extension's tests in a real VSCode

`npm run test:e2e` (see [development.md](development.md)) starts VSCode with the extension and checks, on files checked
out with CRLF, most of what used to be checked by hand: activation with the engine placed in its data directory,
History, commit details and diffs, line staging, discarding lines, Undo and commit through the daemon and on the git
execution layer, and `windowsHide` on every process the extension starts. CI runs it on Windows too, but with
`core.autocrlf false` set globally; the test repository sets `core.autocrlf=true` itself.

`scripts\e2e-windows.ps1` in `apps\vscode` runs it and returns its exit code. With `-Desktop`, it runs it on the desktop
of the user who is logged on, through a one-time scheduled task, and prints its output over ssh as it goes: an ssh
session has no desktop, so use it when VSCode doesn't start from ssh, or to watch VSCode on the screen (Remote Desktop
or in person). The user must be logged on (a locked screen is fine). PowerShell's own window for the task may show
briefly; that is the script, not the extension.

```powershell
cd C:\src\amonhen
cargo build -p amonhen-cli
cd apps\vscode; npm.cmd ci
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\e2e-windows.ps1 -Desktop
```

## Trying an engine built from a branch in VSCode

The extension starts the shared daemon with the `current` engine only when its protocol matches the bundled engine's,
and otherwise with the bundled engine itself. An already running daemon of the same protocol is reused. So an engine
with a new protocol is used only when it is also the bundled one:

1. Build it: `$env:AMONHEN_VERSION = "<a version above the installed one>"; cargo build --release -p amonhen-cli`, then
   `target\release\amonhen.exe install`.
2. If the protocol changed, copy it over the extension's
   `%USERPROFILE%\.vscode\extensions\fukusaka.amonhen-<version>\bin\amonhen-windows-x86_64.exe` (keep the original).
   The extension's own code can be tried the same way: `node esbuild.mjs --production` in `apps\vscode`, then copy
   `dist` over the installed extension's `dist`.
3. Stop the running daemon with the engine it runs (`amonhen daemon stop`; `amonhen daemon status` shows the version),
   then Developer: Reload Window in VSCode.

Installing the next snapshot or release `.vsix` puts everything back.

## What to check on the machine

`npm run test:e2e` covers these (run it with `scripts\e2e-windows.ps1`, above):

- The extension in VSCode: the engine placed in `%LOCALAPPDATA%\amonhen` (`current`, `bin\amonhen.exe`), History,
  commit details and diffs, Working Copy with line staging, discarding lines and Undo, commit, all on a file checked out
  with CRLF.
- With `amonhen.daemon.enabled` off, the same Working Copy operations on the extension's git execution layer.
- Every process the extension starts has `windowsHide` (the cause of console windows flashing), and its resident
  `git cat-file` ends when the repository is closed.

Still by hand:

- No console window flashes while using the extension (selecting branches, refreshing). The tests check only the
  processes the extension starts itself, not those the daemon starts.
- One shared daemon for several VSCode windows and the CLI (`amonhen daemon status` lists the connections).
- Remote-SSH from the Mac to the machine, with the extension running there.
- A Release zip downloaded with a browser (SmartScreen), and `amonhen.exe install`.
