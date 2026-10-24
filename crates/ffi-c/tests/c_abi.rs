#![allow(clippy::disallowed_methods)] // テストの git などはそのまま起動する
use std::ffi::{CStr, CString};
use std::process::Command;

use amonhen_ffi_c::*;

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "a")
        .env("GIT_AUTHOR_EMAIL", "a@example.com")
        .env("GIT_COMMITTER_NAME", "a")
        .env("GIT_COMMITTER_EMAIL", "a@example.com")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
    // テスト用のリポジトリは、CI の環境（利用者名がない・Windows の autocrlf）に左右されないようにする。
    if args.first() == Some(&"init") {
        for (key, value) in [
            ("user.name", "a"),
            ("user.email", "a@example.com"),
            ("core.autocrlf", "false"),
        ] {
            git(dir, &["config", key, value]);
        }
    }
}

#[test]
fn reads_history_through_the_c_abi() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(dir, &["commit", "--allow-empty", "-q", "-m", "first"]);
    git(dir, &["commit", "--allow-empty", "-q", "-m", "second"]);

    unsafe {
        let path = CString::new(dir.to_str().unwrap()).unwrap();
        let repo = amonhen_repo_open(path.as_ptr(), std::ptr::null());
        assert!(!repo.is_null());
        assert!(amonhen_history_acquire(repo).is_null());

        let history = amonhen_history_load(repo);
        let view = amonhen_history_view(history);
        assert_eq!(
            (view.generation, view.row_count, view.edge_count),
            (1, 2, 1)
        );

        let mut out = [0u32; 4];
        assert_eq!(
            amonhen_history_edges_in(history, 0, 2, out.as_mut_ptr(), 4),
            1
        );

        let chunk = amonhen_history_chunk(repo, history, 0, 2, AmonhenEncoding::Utf16);
        let c = amonhen_chunk_view(chunk);
        assert_eq!(c.row_count, 2);
        let s = *c.subjects;
        let bytes = std::slice::from_raw_parts(
            (*c.bases.add(s.buf as usize)).add(s.off as usize),
            s.len() as usize,
        );
        assert_eq!(bytes, b"second");

        amonhen_chunk_free(chunk);
        amonhen_history_release(history);
        amonhen_repo_free(repo);
    }
}

#[test]
fn reports_why_opening_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = CString::new(tmp.path().join("missing").to_str().unwrap()).unwrap();
    unsafe {
        assert!(amonhen_repo_open(path.as_ptr(), std::ptr::null()).is_null());
        assert!(!CStr::from_ptr(amonhen_last_error()).to_bytes().is_empty());
    }
}

#[test]
fn shows_the_cached_history_on_the_next_open() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("repo");
    std::fs::create_dir(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&dir, &["commit", "--allow-empty", "-q", "-m", "first"]);
    let path = CString::new(dir.to_str().unwrap()).unwrap();
    let cache = CString::new(tmp.path().join("cache").to_str().unwrap()).unwrap();
    unsafe {
        let repo = amonhen_repo_open(path.as_ptr(), cache.as_ptr());
        assert!(amonhen_history_load_cached(repo).is_null());
        amonhen_history_release(amonhen_history_load(repo));
        amonhen_repo_free(repo);

        let repo = amonhen_repo_open(path.as_ptr(), cache.as_ptr());
        let cached = amonhen_history_load_cached(repo);
        assert!(!cached.is_null());
        assert_eq!(amonhen_history_view(cached).row_count, 1);
        amonhen_history_release(cached);
        amonhen_repo_free(repo);
    }
}

#[test]
fn lists_refs_and_finds_their_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(dir, &["commit", "--allow-empty", "-q", "-m", "first"]);
    git(dir, &["branch", "topic"]);
    git(dir, &["commit", "--allow-empty", "-q", "-m", "second"]);
    unsafe {
        let path = CString::new(dir.to_str().unwrap()).unwrap();
        let repo = amonhen_repo_open(path.as_ptr(), std::ptr::null());
        let history = amonhen_history_load(repo);
        let refs = amonhen_refs_load(repo);
        let v = amonhen_refs_view(refs);
        assert_eq!(v.count, 2);
        let text = |off: u32, len: u32| {
            std::str::from_utf8(std::slice::from_raw_parts(
                v.strings.add(off as usize),
                len as usize,
            ))
            .unwrap()
        };
        assert_eq!(text(v.head_branch_off, v.head_branch_len), "main");
        let entries = std::slice::from_raw_parts(v.entries, v.count as usize);
        assert_eq!(text(entries[1].name_off, entries[1].name_len), "topic");
        assert_eq!(entries[1].detail_len, u32::MAX);
        assert_eq!(
            amonhen_history_row_of(history, entries[0].target.as_ptr()),
            0
        );
        assert_eq!(
            amonhen_history_row_of(history, entries[1].target.as_ptr()),
            1
        );
        assert_eq!(amonhen_history_row_of(history, [0u8; 20].as_ptr()), -1);

        let again = amonhen_refs_load(repo);
        assert_eq!(
            amonhen_refs_view(again).generation,
            v.generation,
            "変わっていなければ同じ世代"
        );
        amonhen_refs_release(again);
        amonhen_refs_release(refs);
        amonhen_history_release(history);
        amonhen_repo_free(repo);
    }
}

#[test]
fn shows_the_files_a_commit_changed_and_their_diffs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q"]);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "first"]);
    std::fs::write(dir.join("a.txt"), "one\n二\n").unwrap();
    git(dir, &["commit", "-q", "-am", "second"]);
    unsafe {
        let path = CString::new(dir.to_str().unwrap()).unwrap();
        let repo = amonhen_repo_open(path.as_ptr(), std::ptr::null());
        let history = amonhen_history_load(repo);
        let head = amonhen_history_view(history);
        let refs = amonhen_refs_load(repo);
        let target = (*amonhen_refs_view(refs).entries).target;
        assert_eq!(amonhen_history_row_of(history, target.as_ptr()), 0);
        assert_eq!(head.row_count, 2);

        let changes = amonhen_commit_changes(repo, target.as_ptr());
        let cv = amonhen_changes_view(changes);
        assert_eq!(cv.count, 1);
        let e = &*cv.entries;
        assert_eq!(
            std::slice::from_raw_parts(cv.strings.add(e.path_off as usize), e.path_len as usize),
            b"a.txt"
        );
        assert_eq!((e.kind, e.old_path_len), (2, u32::MAX));

        let diff = amonhen_file_diff(repo, changes, 0, AmonhenEncoding::Utf8);
        let dv = amonhen_diff_view(diff);
        assert_eq!(
            (dv.row_count, dv.added, dv.removed, dv.binary),
            (4, 1, 1, 0)
        );
        let rows = std::slice::from_raw_parts(dv.rows, dv.row_count as usize);
        let text = |r: &amonhen_layout::DiffRow| {
            std::str::from_utf8(std::slice::from_raw_parts(
                (*dv.bases.add(r.text.buf as usize)).add(r.text.off as usize),
                r.text.len() as usize,
            ))
            .unwrap()
            .to_owned()
        };
        let lines: Vec<String> = rows.iter().map(text).collect();
        assert_eq!(lines, ["@@ -1,2 +1,2 @@", "one", "two", "二"]);
        assert!(amonhen_file_diff(repo, changes, 9, AmonhenEncoding::Utf8).is_null());

        amonhen_diff_free(diff);
        amonhen_changes_free(changes);
        amonhen_refs_release(refs);
        amonhen_history_release(history);
        amonhen_repo_free(repo);
    }
}
