//! Whether a program has started something that is still running, asked of
//! the system at the moment it matters -- a reader leaving, or closing the
//! terminal -- and at no other.
//!
//! The answer a shell gives itself is better, and is believed where the
//! shell gives one (`Terminal::is_busy`): this is what is left where it
//! does not. Every terminal that asks before closing asks one of these two.
//!
//! On a Unix it is the pty's foreground process group, which a shell hands
//! to each command it runs and takes back when the command ends: a shell
//! waiting for a command is the foreground itself. What was sent to the
//! background is not counted, and was the reader's to leave there.
//!
//! Windows has no foreground and the pseudo-console says nothing of one, so
//! it is whether the program has a child at all. That counts a background
//! job too, and a shell that keeps a helper of its own while it waits, and
//! both of those err the way that asks: a question too many is a key
//! pressed twice, and one too few is a build stopped.

/// Whether the program `pid`, on `master`, has something running under it.
///
/// Yes where it cannot be told, which is the answer that asks.
#[cfg(unix)]
pub(super) fn anything_beneath(pid: u32, master: &dyn portable_pty::MasterPty) -> bool {
    master
        .process_group_leader()
        .is_none_or(|leader| u32::try_from(leader).ok() != Some(pid))
}

/// Whether the program `pid` has something running under it.
///
/// Yes where it cannot be told, which is the answer that asks.
#[cfg(windows)]
pub(super) fn anything_beneath(pid: u32, _: &dyn portable_pty::MasterPty) -> bool {
    use std::os::windows::io::{FromRawHandle as _, OwnedHandle};

    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };

    // Safety: the snapshot is checked before it is used and owned from then
    // on, so it is closed however this returns; the entry is a plain struct
    // whose size is said before the first call reads it, as the calls ask.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return true;
        }
        let _owned = OwnedHandle::from_raw_handle(snapshot);
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX);
        let mut more = Process32FirstW(snapshot, &raw mut entry) != 0;
        while more {
            if entry.th32ParentProcessID == pid && entry.th32ProcessID != pid {
                return true;
            }
            more = Process32NextW(snapshot, &raw mut entry) != 0;
        }
        false
    }
}
