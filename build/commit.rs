// Which build this is, for the first line of the log.
//
// `CARGO_PKG_VERSION` is what a release is numbered, and every build
// between two releases shares it. What a log has to answer is the question
// a reader's report always turns on -- "was the fix in the thing you ran"
// -- and a version that has said `0.1.0` since the first commit cannot.
//
// So the commit, and only the commit.
//
// It used to say whether the tree had anything uncommitted in it too, with
// a `+`, and that was worth having: a `cargo run` from a working tree is
// not the commit it sits on and not anything else either. But it could not
// be known from here. This runs again only when the two files below change,
// and editing a file changes neither -- so the `+` stayed at whatever it
// was when the last commit was made, which is to say almost never set. The
// log then told a reader running edited code that their tree was clean,
// which is the one direction a line like this must not be wrong in.
//
// What is left is what the two files can answer. They change exactly when
// the commit does, so what is watched and what is reported are now the
// same thing.
//
// Nothing here fails a build. A tarball with no `.git`, a machine with no
// git, a checkout of a shallow clone: all of them say so and carry on,
// because what is being built is an editor and not a release process.
use std::process::Command;

/// Says which commit this is, as `OBELUS_BUILD`.
///
/// A function rather than the `main` it used to be: the window's build
/// script has a second errand, and a file included into two others cannot
/// be the whole of either any more.
fn stamp_the_commit() {
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
        Some(commit) if !commit.is_empty() => commit,
        _ => "unknown".to_string(),
    };
    println!("cargo:rustc-env=OBELUS_BUILD={built}");
}
