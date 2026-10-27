//! A file only the person running Obelus can read: the key a server lets
//! windows in with, and the keys a window keeps for the machines it has
//! been drawn from. Either is the whole of what stands between somebody
//! else on the machine and running anything as this person on another.
//!
//! **Private from the moment it has a name.** The file is made beside the
//! one it is for, already private, written, and renamed over it -- so there
//! is no moment a file of that name holds the key and somebody else may
//! read it, and another Obelus writing the same file at the same moment
//! leaves one whole file or the other. A rename keeps the file's own
//! permissions, on both platforms, so what it replaces gives it nothing.
//!
//! **Its owner's alone on Windows too.** A file there takes the permissions
//! of the directory it is made in, which under the reader's own profile is
//! already only theirs -- and anywhere else, a `XDG_STATE_HOME` pointed at
//! `C:\tmp` say, is whatever that directory lets everybody do. So the list
//! of who may is said outright and inherits nothing: the person this
//! process runs as, and nobody else. That person and not the file's owner,
//! because an elevated process makes the Administrators group the owner of
//! what it writes, and that group is not one person.

use std::{io, path::Path};

/// Writes `bytes` to `path` for nobody but the person running Obelus.
///
/// # Errors
///
/// Where the file could not be made or written beside `path`, or not
/// renamed over it -- and then nothing is left beside it either.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::{
        io::Write as _,
        sync::atomic::{AtomicU64, Ordering},
    };

    // This process's own name beside it, and this write's own within the
    // process: two writers sharing one name truncate each other's
    // half-written file, and the first rename takes the other's away.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".writing.{}.{}",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let beside = path.with_file_name(name);
    let written = made(&beside)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| std::fs::rename(&beside, path));
    if written.is_err() {
        // The name is this process's own, so nobody else would ever take
        // it away.
        let _ = std::fs::remove_file(&beside);
    }
    written
}

/// A new file at `path` that only its owner may read or write.
#[cfg(unix)]
fn made(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    // At the open and not after it: a mode set once the file is there is a
    // moment it was there with the umask's.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// A new file at `path` that only the person this process runs as may do
/// anything with -- said when it is made, so that it never had the
/// directory's.
#[cfg(windows)]
fn made(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::{ffi::OsStrExt as _, io::FromRawHandle as _};

    use windows_sys::Win32::{
        Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL},
    };

    // `P`, protected: nothing inherited from the directory. And one entry,
    // all access for this person.
    let said: Vec<u16> = format!("D:P(A;;FA;;;{})", windows::user()?)
        .encode_utf16()
        .chain([0])
        .collect();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // Safety: both strings end in a nul and outlive the calls; the
    // descriptor is the system's until it is given back below, and the
    // attributes that point at it are this frame's and read only by the
    // call that makes the file.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            said.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            &raw const attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        let made = io::Error::last_os_error();
        LocalFree(descriptor);
        if handle == INVALID_HANDLE_VALUE {
            return Err(made);
        }
        Ok(std::fs::File::from_raw_handle(handle))
    }
}

/// Whether nobody but the person running Obelus may read `path` -- what
/// [`write`] makes, asked of the file as the system has it rather than of
/// how it was made, so a test of whoever writes one can ask it.
///
/// # Errors
///
/// Where the system would not say.
#[cfg(unix)]
pub fn is_private(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt as _;

    Ok(std::fs::metadata(path)?.permissions().mode() & 0o077 == 0)
}

/// The same: one entry, all access for this person, and nothing taken from
/// the directory -- so a directory changed later changes nothing here.
///
/// # Errors
///
/// Where the system would not say.
#[cfg(windows)]
pub fn is_private(path: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetSecurityDescriptorControl,
            PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut list: *mut ACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // Safety: the path ends in a nul; the list points into the descriptor,
    // which is the system's until it is given back at the end, and every
    // entry is read through the call that found it.
    let (protected, entries) = unsafe {
        let asked = GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut list,
            std::ptr::null_mut(),
            &raw mut descriptor,
        );
        if asked != 0 {
            return Err(io::Error::from_raw_os_error(
                i32::try_from(asked).unwrap_or(i32::MAX),
            ));
        }
        let (mut control, mut revision) = (0, 0);
        GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision);
        let mut size: ACL_SIZE_INFORMATION = std::mem::zeroed();
        GetAclInformation(
            list,
            (&raw mut size).cast(),
            u32::try_from(std::mem::size_of::<ACL_SIZE_INFORMATION>()).unwrap_or(u32::MAX),
            AclSizeInformation,
        );
        let mut entries = Vec::new();
        for at in 0..size.AceCount {
            let mut entry = std::ptr::null_mut();
            if GetAce(list, at, &raw mut entry) == 0 {
                continue;
            }
            let entry = &*entry.cast::<ACCESS_ALLOWED_ACE>();
            // Zero is an entry that allows; one that denies has the same
            // shape and the opposite meaning.
            let allows = entry.Header.AceType == 0;
            let sid = windows::sid((&raw const entry.SidStart).cast_mut().cast());
            entries.push((allows, sid, entry.Mask));
        }
        LocalFree(descriptor);
        (control & SE_DACL_PROTECTED != 0, entries)
    };
    let user = windows::user()?;
    Ok(protected
        && matches!(
            entries.as_slice(),
            [(true, Ok(sid), FILE_ALL_ACCESS)] if *sid == user
        ))
}

/// Who a process is, the way Windows says it.
#[cfg(windows)]
mod windows {
    use std::io;

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, PSID, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    /// The person this process runs as, as the letters of their SID.
    pub(crate) fn user() -> io::Result<String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // Safety: the token is this frame's and closed before it returns;
        // the buffer is as large as the first call said, aligned for what
        // the second writes into it, and read only after that succeeded.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut size = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut size);
            let mut buffer = vec![0_u64; usize::try_from(size).unwrap_or(0).div_ceil(8)];
            let asked = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &raw mut size,
            );
            let answered = io::Error::last_os_error();
            CloseHandle(token);
            if asked == 0 {
                return Err(answered);
            }
            sid(buffer.as_ptr().cast::<TOKEN_USER>().read().User.Sid)
        }
    }

    /// The letters of a SID.
    pub(crate) fn sid(sid: PSID) -> io::Result<String> {
        let mut letters: *mut u16 = std::ptr::null_mut();
        // Safety: the letters are the system's, nul-terminated, read up to
        // the nul and given back.
        unsafe {
            if ConvertSidToStringSidW(sid, &raw mut letters) == 0 {
                return Err(io::Error::last_os_error());
            }
            let length = (0..).take_while(|&at| *letters.add(at) != 0).count();
            let said = String::from_utf16_lossy(std::slice::from_raw_parts(letters, length));
            LocalFree(letters.cast());
            Ok(said)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of the test's own, gone when it is.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("obelus-private-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("a scratch directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What is written is there, over what was, and nothing is left beside
    /// it.
    ///
    /// Broken deliberately by leaving out the rename in `write`: the old
    /// bytes stayed where they were, and the new ones in a file beside them.
    #[test]
    fn what_is_written_replaces_what_was() {
        let scratch = Scratch::new("replaces");
        let path = scratch.0.join("server.key");
        std::fs::write(&path, "old").expect("the old file");
        write(&path, b"new").expect("written");
        assert_eq!(std::fs::read(&path).expect("read back"), b"new");
        let names: Vec<_> = std::fs::read_dir(&scratch.0)
            .expect("the directory")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(names, ["server.key"], "something was left beside it");
    }

    /// What is written is private, and a file written the ordinary way
    /// beside it is not -- which is what says `is_private` asks the file and
    /// does not just answer yes.
    ///
    /// Broken deliberately by making the file the ordinary way in `made`
    /// (no mode on unix; a null `lpSecurityDescriptor` on Windows): it took
    /// the umask's, or what the temporary directory hands down, and was
    /// not private.
    #[test]
    fn what_is_written_is_private_and_an_ordinary_file_is_not() {
        let scratch = Scratch::new("private");
        let path = scratch.0.join("server.key");
        write(&path, b"key").expect("written");
        assert!(is_private(&path).expect("asked"), "anybody may read it");

        let ordinary = scratch.0.join("ordinary");
        std::fs::write(&ordinary, "anybody's").expect("written");
        assert!(
            !is_private(&ordinary).expect("asked"),
            "an ordinary file was taken for a private one"
        );
    }
}
