//! What `obg` says it is to Windows.

/// `obg` is a window and not a console program, so that starting it from a
/// menu does not put up a console beside it.
///
/// Read out of the executable itself, where Windows reads it: the PE header's
/// `Subsystem`, which is 2 for a window and 3 for a console.
///
/// Broken deliberately by taking `windows_subsystem` out of `main.rs`: the
/// field is 3.
#[cfg(windows)]
#[test]
fn obg_is_a_window_and_not_a_console_program() {
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_obg")).expect("the executable");
    let at = |offset: usize, width: usize| {
        bytes[offset..offset + width]
            .iter()
            .rev()
            .fold(0usize, |value, byte| value << 8 | usize::from(*byte))
    };
    // The DOS header says where the PE header is; after its signature come
    // the 20 bytes of the file header, and the subsystem is 68 bytes into
    // the optional header after that.
    let pe = at(0x3c, 4);
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "not a PE file");
    assert_eq!(
        at(pe + 4 + 20 + 68, 2),
        2,
        "obg says it is a console program"
    );
}
