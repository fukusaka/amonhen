//! Windows の SSH 先（OpenSSH サーバ）。送ったコマンドは、レジストリの `DefaultShell` のシェル（なければ cmd.exe。
//! pwsh・Windows PowerShell のことも多い）が実行するので、sh の文は使えない（`docs/design.md` の「SSH hosts running
//! Windows」）。
//!
//! - 調べる・片付ける・送ったものを仕上げるのは PowerShell のスクリプト。`powershell -EncodedCommand` で渡すと、どの
//!   シェルからでも同じ意味になる。
//! - 送るのは scp（Windows PowerShell 5.1 は、標準入力を 1 MB ほど読んだところで止まる）。
//! - エンジンは既定のシェルが直接起動する（セッションの標準入出力を、そのまま渡す）。

use amonhen_model::{Result, tr};

/// SSH 先の既定のシェル。エンジンを起動するコマンドの書き方が違う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Cmd,
    PowerShell,
}

impl Shell {
    /// `DefaultShell` の値から（空なら cmd.exe）。
    pub fn from_default_shell(value: &str) -> Self {
        let name = value
            .trim()
            .trim_matches('"')
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match name.as_str() {
            "pwsh.exe" | "pwsh" | "powershell.exe" | "powershell" => Shell::PowerShell,
            _ => Shell::Cmd,
        }
    }

    /// `program` を `args` で起動するコマンド。cmd.exe は `/c` が外側の引用符の組を外すので、もう 1 組で囲む。
    /// cmd.exe では `%` を逃がせないので、`%` や `"` を含むものは断る。
    pub fn command(self, program: &str, args: &[&str]) -> Result<String> {
        let words = std::iter::once(program).chain(args.iter().copied());
        match self {
            Shell::PowerShell => Ok(format!(
                "& {}",
                words.map(ps_quote).collect::<Vec<_>>().join(" ")
            )),
            Shell::Cmd => {
                let mut quoted = Vec::new();
                for word in words {
                    if word.contains(['"', '%']) {
                        return Err(tr!(
                            "Can’t pass {path} to cmd.exe on the SSH host (it has \" or %)",
                            path = word
                        )
                        .into());
                    }
                    quoted.push(format!("\"{word}\""));
                }
                Ok(format!("\"{}\"", quoted.join(" ")))
            }
        }
    }
}

/// どのスクリプトにも先に付ける。進捗の表示（モジュールを読み込むときの CLIXML）は出さない。モジュールの場所は
/// Windows PowerShell のものに戻す（pwsh から cmd.exe などを挟んで起動されると pwsh のものを引き継ぎ、
/// `Get-FileHash` などのコマンドが見つからない）。
const PREAMBLE: &str = "$ProgressPreference = 'SilentlyContinue'\n$env:PSModulePath = \"$PSHOME\\Modules;\" + [Environment]::GetEnvironmentVariable('PSModulePath', 'Machine')\n";

/// `script` を、どのシェルからでも同じ意味になる形で（`-EncodedCommand` は UTF-16LE の base64）。
pub fn encoded(script: &str) -> String {
    let script = format!("{PREAMBLE}{script}");
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64(&bytes)
    )
}

/// PowerShell の一重引用符の文字列。
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 調べて片付ける。sh の調べ（`ssh::prepare`）と同じ行を出し、5 行目に `DefaultShell` を足す:
/// `windows <CPU>`、エンジンの置き場所、PATH の amonhen、その版、`DefaultShell`、置いてあるエンジン（`<ハッシュ> <ビルド>`）。
/// このビルドのエンジンには使った印を付け、`keep_days` 日使われていないもの（動いていて消せないものは除く）と、
/// 1 日たった書きかけを消す。
pub fn probe_script(build: &str, build_file: &str, keep_days: u32) -> String {
    format!(
        r#"$ErrorActionPreference = 'SilentlyContinue'
'windows ' + $env:PROCESSOR_ARCHITECTURE
$d = Join-Path $env:LOCALAPPDATA 'amonhen\engine'
$d
$a = Get-Command amonhen -CommandType Application | Select-Object -First 1
if ($a) {{ $a.Source; & $a.Source --version 2>$null | Select-Object -First 1 }} else {{ ''; '' }}
"$((Get-ItemProperty 'HKLM:\SOFTWARE\OpenSSH').DefaultShell)"
$old = (Get-Date).AddDays(-{keep_days})
foreach ($e in Get-ChildItem $d -Directory) {{
  $x = Join-Path $e.FullName 'amonhen.exe'
  $b = "$(Get-Content (Join-Path $e.FullName '{build_file}') -Raw)".Trim()
  $u = Join-Path $e.FullName 'used'
  if ($b -eq {build} -or -not (Test-Path $u)) {{ Set-Content $u '' }}
  elseif ((Get-Item $u).LastWriteTime -lt $old) {{
    try {{ [IO.File]::Open($x, 'Open', 'ReadWrite', 'None').Close(); Remove-Item $e.FullName -Recurse -Force; continue }} catch {{}}
  }}
  Get-ChildItem $e.FullName -Filter 'amonhen.tmp.*' | Where-Object LastWriteTime -lt (Get-Date).AddDays(-1) | Remove-Item -Force
  if (Test-Path $x) {{ $e.Name + ' ' + $b }}
}}"#,
        build = ps_quote(build),
    )
}

/// 送る前にフォルダを作る。
pub fn mkdir_script(dir: &str) -> String {
    format!(
        "New-Item -ItemType Directory -Force -Path {} | Out-Null",
        ps_quote(dir)
    )
}

/// scp で `tmp` に送ったものを `amonhen.exe` にし、ビルドの指紋と使った印を書いて、SHA-1 を出す。
pub fn finish_script(dir: &str, tmp: &str, build: &str, build_file: &str) -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
$d = {dir}
Move-Item -Force (Join-Path $d {tmp}) (Join-Path $d 'amonhen.exe')
Set-Content (Join-Path $d '{build_file}') {build} -NoNewline
Set-Content (Join-Path $d 'used') ''
$sha = [Security.Cryptography.SHA1]::Create()
[BitConverter]::ToString($sha.ComputeHash([IO.File]::ReadAllBytes((Join-Path $d 'amonhen.exe')))).Replace('-', '').ToLower()"#,
        dir = ps_quote(dir),
        tmp = ps_quote(tmp),
        build = ps_quote(build),
    )
}

/// 送るのをやめたとき（中身が合わなかった）、そのフォルダを消す。
pub fn remove_script(dir: &str) -> String {
    format!(
        "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue {}",
        ps_quote(dir)
    )
}

/// `ssh://host/<path>` の `<path>` を、SSH 先のエンジンに渡す形に: `/C:/src` は `C:/src`、`/~/src` は `~/src`
/// （エンジンがホームから読む）。
pub fn engine_path(path: &str) -> String {
    let rest = path.strip_prefix('/').unwrap_or(path);
    let bytes = rest.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive || rest == "~" || rest.starts_with("~/") {
        rest.to_owned()
    } else {
        path.to_owned()
    }
}

/// エンジンの置き場所の中の、ハッシュの名前のフォルダ。
pub fn engine_dir(dir: &str, hash: &str) -> String {
    format!("{}\\{hash}", dir.trim_end_matches(['\\', '/']))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_commands_for_the_default_shell() {
        assert_eq!(Shell::from_default_shell(""), Shell::Cmd);
        assert_eq!(
            Shell::from_default_shell(r"C:\Windows\System32\cmd.exe"),
            Shell::Cmd
        );
        assert_eq!(
            Shell::from_default_shell(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            Shell::PowerShell
        );
        assert_eq!(
            Shell::from_default_shell(r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe"),
            Shell::PowerShell
        );
        let exe = r"C:\Users\Ada Lovelace\AppData\Local\amonhen\engine\0123\amonhen.exe";
        assert_eq!(
            Shell::Cmd
                .command(exe, &["--stdio", "C:/src/it's"])
                .unwrap(),
            r#"""C:\Users\Ada Lovelace\AppData\Local\amonhen\engine\0123\amonhen.exe" "--stdio" "C:/src/it's"""#
        );
        assert_eq!(
            Shell::PowerShell
                .command(exe, &["--stdio", "C:/src/it's"])
                .unwrap(),
            r"& 'C:\Users\Ada Lovelace\AppData\Local\amonhen\engine\0123\amonhen.exe' '--stdio' 'C:/src/it''s'"
        );
        assert!(Shell::Cmd.command(exe, &["C:/100%"]).is_err());
        assert!(Shell::PowerShell.command(exe, &["C:/100%"]).is_ok());
    }

    #[test]
    fn encodes_scripts_for_powershell() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // UTF-16LE の base64。
        assert!(
            encoded("").ends_with(&base64(
                &PREAMBLE
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            ))
        );
        // cmd.exe のコマンドは 8191 文字まで。
        let probe = encoded(&probe_script("0123456789abcdef", "build", 30));
        assert!(probe.len() < 8000, "{}", probe.len());
    }

    #[test]
    fn passes_location_paths_to_the_engine() {
        assert_eq!(engine_path("/C:/src/repo"), "C:/src/repo");
        assert_eq!(engine_path("/c:"), "c:");
        assert_eq!(engine_path("/~/src"), "~/src");
        assert_eq!(engine_path("/~"), "~");
        assert_eq!(engine_path("/srv/repo"), "/srv/repo");
        assert_eq!(
            engine_dir(r"C:\Users\ada\AppData\Local\amonhen\engine", "0123"),
            r"C:\Users\ada\AppData\Local\amonhen\engine\0123"
        );
    }
}
