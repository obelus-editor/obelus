//! The clipboards that are a system service, asked directly.
//!
//! Three platforms, two answers. On Linux the clipboard is a *protocol*
//! between clients -- there is no server, the content belongs to whoever
//! last copied, and it dies with them -- so Obelus asks the program the
//! machine already has for exactly that reason (`wl-copy` forks and holds
//! it, which is why a copy there survives Obelus quitting). That is the
//! module around this one.
//!
//! On macOS and Windows the clipboard *is* a service. The content is the
//! system's the moment it is handed over, it outlives every process without
//! anybody holding it, and it is asked for through a call rather than
//! through a program. Which matters for more than tidiness: `pbpaste` can
//! hand over text and nothing else, and `win32yank.exe` is something a
//! reader installs for WSL and does not have -- so on a plain Windows
//! machine the fallback was an escape sequence written to a stdout that is
//! not a terminal, which is to say nothing at all.
//!
//! What every platform says here is said in mime types, because the one
//! above it asks in mime types. A UTI and a numbered clipboard format are
//! each platform's own word for the same few things, and translating them
//! here is what keeps the callers from learning three vocabularies.

/// What the clipboard is offering, or `None` where this platform has no
/// service to ask.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn types() -> Option<Vec<String>> {
    None
}

/// The same, for one shape.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn paste_as(_mime: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(target_os = "macos")]
pub(crate) use cocoa::{paste_as, types};
#[cfg(windows)]
pub(crate) use win32::{paste_as, types};

/// macOS: the general pasteboard.
#[cfg(target_os = "macos")]
mod cocoa {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSArray, NSString, NSURL};

    /// What is on it, said in mime types.
    ///
    /// Only the two Obelus asks about. The pasteboard names dozens of
    /// shapes -- a copied file carries its name, its icon and its
    /// promises -- and reporting all of them would be reporting a
    /// vocabulary nothing here speaks.
    pub(crate) fn types() -> Option<Vec<String>> {
        let mut names = Vec::new();
        if !urls().is_empty() {
            names.push(super::super::FILES.to_string());
        }
        if string().is_some() {
            names.push("text/plain".to_string());
        }
        Some(names)
    }

    /// One shape, as bytes.
    pub(crate) fn paste_as(mime: &str) -> Option<Vec<u8>> {
        if mime == super::super::FILES {
            let list: Vec<String> = urls().into_iter().map(|url| url.to_string()).collect();
            return (!list.is_empty()).then(|| list.join("\r\n").into_bytes());
        }
        if mime.starts_with("text/") {
            return string().map(String::into_bytes);
        }
        None
    }

    /// The files on the pasteboard, as `file://` URIs.
    ///
    /// `readObjectsForClasses` is the documented way to ask: a copy from
    /// the Finder is a list of `NSURL`, and a copy of text is not, so an
    /// empty answer is how "no files" arrives.
    fn urls() -> Vec<String> {
        // Safety: the general pasteboard is a singleton that exists for
        // the life of the process, and every object below is read and
        // dropped inside this function.
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            let classes = NSArray::from_slice(&[NSURL::class()]);
            let Some(objects) = pasteboard.readObjectsForClasses_options(&classes, None) else {
                return Vec::new();
            };
            objects
                .iter()
                .filter_map(|object| object.downcast::<NSURL>().ok())
                .filter(|url| url.isFileURL())
                .filter_map(|url| url.absoluteString().map(|string| string.to_string()))
                .collect()
        }
    }

    /// And the text on it.
    fn string() -> Option<String> {
        // Safety: as above.
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            let said: Option<objc2::rc::Retained<NSString>> =
                pasteboard.stringForType(NSPasteboardTypeString);
            said.map(|said| said.to_string())
        }
    }
}

/// Windows: the clipboard the window manager keeps.
#[cfg(windows)]
mod win32 {
    use std::{os::windows::ffi::OsStringExt, path::PathBuf};

    use windows_sys::Win32::{
        Foundation::{HANDLE, HWND},
        System::{
            DataExchange::{
                CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
            },
            Memory::{GlobalLock, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
        UI::Shell::{DragQueryFileW, HDROP},
    };

    /// The format a list of dropped files arrives in, which is what a copy
    /// in Explorer puts on the clipboard.
    const CF_HDROP: u32 = 15;

    /// Holds the clipboard open, and lets it go however the reading ends.
    ///
    /// The API is a lock: nothing else on the machine can read or write
    /// while it is open, so a path out of a function that forgot to close
    /// it is a clipboard the whole desktop has lost.
    struct Open;

    impl Open {
        fn taken() -> Option<Self> {
            // Safety: a null window is documented as "this task", which is
            // what a program with no window of its own wants -- and `obg`'s
            // window belongs to another thread.
            let opened = unsafe { OpenClipboard(std::ptr::null_mut::<HWND>() as HWND) };
            (opened != 0).then_some(Self)
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            // Safety: only ever reached with the clipboard open.
            unsafe { CloseClipboard() };
        }
    }

    /// What is on it, said in mime types.
    pub(crate) fn types() -> Option<Vec<String>> {
        let _open = Open::taken()?;
        let mut names = Vec::new();
        // Safety: asking whether a format is there reads nothing and
        // cannot fail in a way that matters.
        if unsafe { IsClipboardFormatAvailable(CF_HDROP) } != 0 {
            names.push(super::super::FILES.to_string());
        }
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT as u32) } != 0 {
            names.push("text/plain".to_string());
        }
        Some(names)
    }

    /// One shape, as bytes.
    pub(crate) fn paste_as(mime: &str) -> Option<Vec<u8>> {
        let _open = Open::taken()?;
        if mime == super::super::FILES {
            let list: Vec<String> = dropped()
                .into_iter()
                .map(|path| format!("file://{}", path.display().to_string().replace('\\', "/")))
                .collect();
            return (!list.is_empty()).then(|| list.join("\r\n").into_bytes());
        }
        if mime.starts_with("text/") {
            return text().map(String::into_bytes);
        }
        None
    }

    /// The text on the clipboard, as UTF-16 turned into a string.
    fn text() -> Option<String> {
        // Safety: the handle belongs to the clipboard and is only read
        // while it is open and locked, which is the documented contract.
        unsafe {
            let handle: HANDLE = GetClipboardData(CF_UNICODETEXT as u32);
            if handle.is_null() {
                return None;
            }
            let locked = GlobalLock(handle.cast()).cast::<u16>();
            if locked.is_null() {
                return None;
            }
            let mut length = 0;
            while *locked.add(length) != 0 {
                length += 1;
            }
            let said = String::from_utf16_lossy(std::slice::from_raw_parts(locked, length));
            GlobalUnlock(handle.cast());
            Some(said)
        }
    }

    /// And the files on it.
    fn dropped() -> Vec<PathBuf> {
        // Safety: as above. `DragQueryFileW` with `u32::MAX` as the index
        // is the documented way to ask how many there are, and with a null
        // buffer how long each name is.
        unsafe {
            let handle: HANDLE = GetClipboardData(CF_HDROP);
            if handle.is_null() {
                return Vec::new();
            }
            let drop: HDROP = handle.cast();
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            (0..count)
                .filter_map(|at| {
                    let length = DragQueryFileW(drop, at, std::ptr::null_mut(), 0);
                    if length == 0 {
                        return None;
                    }
                    // One more for the terminator the call writes.
                    let mut name = vec![0u16; length as usize + 1];
                    let written = DragQueryFileW(drop, at, name.as_mut_ptr(), length + 1);
                    name.truncate(written as usize);
                    Some(PathBuf::from(std::ffi::OsString::from_wide(&name)))
                })
                .collect()
        }
    }
}
