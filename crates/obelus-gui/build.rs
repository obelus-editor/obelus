//! Which build this is, and the icon Windows draws for it.
//!
//! The commit is shared with `ob`, whose file says why.
include!("../../build/commit.rs");

fn main() {
    stamp_the_commit();
    embed_the_icon();
}

/// The icon, into the executable's own resources.
///
/// Windows has nowhere else to look. Explorer, the taskbar and the title
/// bar all read a program's icon out of the program itself rather than out
/// of anything beside it -- there is no `.desktop` file to point at one and
/// no bundle to keep one in -- and a build script is the only place that
/// can put one there.
///
/// The `cfg` is the *host's*, because that is what a build script is
/// compiled for and what decides whether the crate below is in the tree at
/// all. So cross-compiling to Windows from somewhere else makes a binary
/// with no icon and says nothing about it. True, and left alone: every
/// Windows build Obelus ships is made on Windows.
#[cfg(windows)]
fn embed_the_icon() {
    let icon = "../../contrib/desktop/obelus.ico";
    println!("cargo:rerun-if-changed={icon}");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon);
    // Nothing here fails a build, for the reason the commit above is not
    // allowed to either: what is being built is an editor, and a
    // Windows SDK that cannot be found is a worse icon rather than no
    // program.
    if let Err(error) = resource.compile() {
        println!("cargo:warning=the icon did not go into the executable: {error}");
    }
}

#[cfg(not(windows))]
fn embed_the_icon() {}
