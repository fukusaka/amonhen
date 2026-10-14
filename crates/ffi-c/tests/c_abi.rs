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
}

#[test]
fn reads_history_through_the_c_abi() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
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
    git(&dir, &["init", "-q", "-b", "main"]);
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
