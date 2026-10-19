//! A binary of its own because what it sets -- where the process keeps its
//! state -- is set once and never set back, and the library's own tests ask
//! where the reader's is.

/// The state directory a test names starts empty, whatever an earlier run
/// that was given the same process number left in it.
///
/// Broken deliberately by setting it without emptying it, as it was: the
/// note left there is still there.
#[test]
fn a_state_directory_for_a_test_starts_empty() {
    let directory =
        std::env::temp_dir().join(format!("obelus-logging-state-{}", std::process::id()));
    let left = directory.join("todo").join("left.toml");
    std::fs::create_dir_all(directory.join("todo")).expect("somewhere to leave it");
    std::fs::write(&left, "said = \"the first\"\n").expect("what was left");

    obelus_logging::state_directory_for_test(directory.clone());
    assert!(!left.exists(), "what an earlier run left is still there");
    assert_eq!(obelus_logging::state_directory(), Some(directory.clone()));
    let _ = std::fs::remove_dir_all(&directory);
}
