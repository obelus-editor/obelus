//! Which build this is, for the first line of the log.
//!
//! `CARGO_PKG_VERSION` is what a release is numbered, and every build
//! between two releases shares it. What a log has to answer is the question
//! a reader's report always turns on -- "was the fix in the thing you ran"
//! -- and a version that has said `0.1.0` since the first commit cannot.
//!
//! So the commit, and whether the tree it was built from had anything
//! uncommitted in it. The second half matters as much as the first: a
//! `cargo run` from a working tree is not the commit it sits on and not
//! anything else either, and a log that named only the commit would say
//! something precise and wrong.
//!
//! Nothing here fails a build. A tarball with no `.git`, a machine with no
//! git, a checkout of a shallow clone: all of them say so and carry on,
//! because what is being built is a code reader and not a release process.
use std::process::Command;

fn main() {
    // A new commit moves the branch's ref; a different branch moves `HEAD`.
    // Neither is watched by default, so without these the log goes on
    // naming the commit this was first built at.
    let git = std::path::Path::new("../../.git");
    println!("cargo:rerun-if-changed={}", git.join("HEAD").display());
    if let Some(reference) = std::fs::read_to_string(git.join("HEAD"))
        .ok()
        .and_then(|head| head.strip_prefix("ref: ").map(|at| at.trim().to_string()))
    {
        println!("cargo:rerun-if-changed={}", git.join(reference).display());
    }

    let said = |arguments: &[&str]| {
        Command::new("git")
            .args(arguments)
            .output()
            .ok()
            .filter(|ran| ran.status.success())
            .map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string())
    };
    let built = match said(&["rev-parse", "--short", "HEAD"]) {
        Some(commit) if !commit.is_empty() => match said(&["status", "--porcelain"]) {
            // Built from a working tree with something in it that is not
            // in the commit. Which is most builds during a day's work, and
            // the one case a bare commit would be a lie about.
            Some(changes) if !changes.is_empty() => format!("{commit}+"),
            _ => commit,
        },
        _ => "unknown".to_string(),
    };
    println!("cargo:rustc-env=OBELUS_BUILD={built}");
}
