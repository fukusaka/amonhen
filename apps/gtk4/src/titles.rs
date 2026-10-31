//! 右クリックとドラッグのメニューの、対象の入った名前（macOS 版の `Titles`）。同じ操作はどこでも同じ言い方に
//! する。シートを開くものは「…」で終える。

use amonhen_model::tr;

/// 短いコミット ID の桁（どこでも同じ）。
pub const SHORT_ID: usize = 8;

pub fn short(oid: &str) -> &str {
    &oid[..oid.len().min(SHORT_ID)]
}

pub fn checkout(name: &str) -> String {
    tr!("Checkout {name}…", name = name)
}

pub fn checkout_as_local(remote: &str) -> String {
    tr!("Checkout {name} as Local Branch…", name = remote)
}

pub fn checkout_detached(oid: &str) -> String {
    tr!("Checkout {id} (Detached)…", id = short(oid))
}

pub fn merge(rev: &str, into: &str) -> String {
    tr!("Merge {rev} into {target}…", rev = rev, target = into)
}

pub fn rebase(target: &str, onto: &str) -> String {
    tr!("Rebase {target} onto {rev}…", target = target, rev = onto)
}

pub fn push(branch: &str, to: &str) -> String {
    tr!("Push {branch} to {target}…", branch = branch, target = to)
}

pub fn force_push(branch: &str, to: &str) -> String {
    tr!(
        "Force Push {branch} to {target}…",
        branch = branch,
        target = to
    )
}

pub fn apply(stash: &str) -> String {
    tr!("Apply {name}…", name = stash)
}

pub fn pop(stash: &str) -> String {
    tr!("Pop {name}…", name = stash)
}

pub fn drop(stash: &str) -> String {
    tr!("Drop {name}…", name = stash)
}

pub fn new_branch_from(start: &str) -> String {
    tr!("New Branch from {name}…", name = start)
}

pub fn cherry_pick(oid: &str) -> String {
    tr!("Cherry-pick {id}…", id = short(oid))
}

pub fn revert(oid: &str) -> String {
    tr!("Revert {id}…", id = short(oid))
}

pub fn delete(name: &str) -> String {
    tr!("Delete {name}…", name = name)
}
