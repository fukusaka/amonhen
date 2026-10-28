//! コミットのヘッダーから署名の種類を読む（検証はしない）。
#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use amonhen_backend_gix::GixBackend;
use amonhen_model::{CommitText, GitBackend, Oid};

fn git(dir: &Path, args: &[&str], input: Option<&str>) -> String {
    amonhen_model::testing::isolate_from_user_settings();
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(input.unwrap_or_default().as_bytes())
        .unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn reads_the_kind_of_signature_from_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"], None);
    let tree = git(dir, &["mktree"], Some(""));
    let header = format!(
        "tree {tree}\nauthor a <a@x> 1700000000 +0000\ncommitter a <a@x> 1700000000 +0000\n"
    );
    let make = |sig: Option<&str>| {
        let mut object = header.clone();
        if let Some(sig) = sig {
            object.push_str(&format!(
                "gpgsig -----BEGIN {sig}-----\n abc\n -----END {sig}-----\n"
            ));
        }
        object.push_str("\nsubject\n");
        let hex = git(
            dir,
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            Some(&object),
        );
        Oid::from_hex(&hex).unwrap()
    };
    let ids = [
        make(None),
        make(Some("PGP SIGNATURE")),
        make(Some("SSH SIGNATURE")),
        make(Some("SIGNED MESSAGE")),
    ];
    let backend = GixBackend::open(dir).unwrap();
    let kinds: Vec<u8> = backend
        .commit_texts(&ids)
        .unwrap()
        .iter()
        .map(|t| t.signature)
        .collect();
    assert_eq!(
        kinds,
        [
            0,
            CommitText::SIGNED_GPG,
            CommitText::SIGNED_SSH,
            CommitText::SIGNED_X509
        ]
    );
    assert_eq!(
        backend.commit_texts(&ids[1..2]).unwrap()[0].subject(),
        b"subject"
    );
}
