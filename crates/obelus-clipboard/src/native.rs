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
//! And one call puts a copy on it in as many shapes as it was made in. So
//! those two need no owner, no hand-over, and no window -- `ob` in a
//! terminal there offers Obelus's own shape exactly as `obg` does, which is
//! why `native::copy` stands in front of both. Owning the selection is
//! Linux's problem alone.
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

/// Puts a copy on it, in every shape it was made in.
///
/// `false` where this platform has no service to put it on, which sends
/// the copy back to the programs.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn copy(_shapes: &[(String, Vec<u8>)]) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub(crate) use cocoa::{copy, paste_as, types};
#[cfg(windows)]
pub(crate) use win32::{copy, paste_as, types};

/// Whether a shape is the words, under one of the names they go by.
///
/// Every one of them is the same bytes, and both services below have one
/// place to put words: a copy that wrote them three times would be three
/// writes of one thing, and on Windows the second would replace the first.
#[cfg(any(target_os = "macos", windows))]
fn is_words(mime: &str) -> bool {
    mime.starts_with("text/") || mime == "UTF8_STRING"
}

/// What a picture on either service is called here, whatever shape it was
/// put there in.
///
/// One name, because each of them holds a picture in a shape no agent is
/// sent -- a DIB on Windows, a TIFF on macOS -- as often as in a PNG, and
/// makes it a PNG on the way out.
#[cfg(any(target_os = "macos", windows))]
const PICTURE: &str = "image/png";

/// Which of the shapes Obelus asks about are on the clipboard.
#[cfg(any(target_os = "macos", windows))]
struct Holding {
    /// A list of files.
    files: bool,
    /// Words.
    words: bool,
    /// A picture, in any shape.
    picture: bool,
}

#[cfg(any(target_os = "macos", windows))]
impl Holding {
    /// Said in mime types.
    ///
    /// A picture only where there are no words beside it. On both services
    /// a program copying words puts a picture of them down too -- a
    /// spreadsheet's cells, a paragraph out of a word processor -- so a
    /// copy that has words was a copy of words, and pasting it as a picture
    /// would send an agent a screenshot of the text the reader meant to
    /// type. A screenshot, or a picture copied out of a browser, has none.
    fn names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.files {
            names.push(super::FILES.to_string());
        }
        if self.words {
            names.push("text/plain".to_string());
        } else if self.picture {
            names.push(PICTURE.to_string());
        }
        names
    }
}

/// A picture the service held in another shape, as the PNG it is sent in.
#[cfg(any(target_os = "macos", windows))]
fn as_png(picture: &image::DynamicImage) -> Option<Vec<u8>> {
    let mut png = std::io::Cursor::new(Vec::new());
    match picture.write_to(&mut png, image::ImageFormat::Png) {
        Ok(()) => Some(png.into_inner()),
        Err(error) => {
            tracing::warn!(%error, "making a pasted picture a PNG");
            None
        }
    }
}

#[cfg(all(test, any(target_os = "macos", windows)))]
mod tests {
    use super::Holding;

    /// A picture with words beside it is the words, and one on its own is
    /// a picture.
    ///
    /// Deliberate break: `names` pushing the picture whenever there is one,
    /// which makes a spreadsheet's cells a screenshot of them.
    #[test]
    fn a_picture_beside_words_is_the_words() {
        let held = |words, picture| {
            Holding {
                files: false,
                words,
                picture,
            }
            .names()
        };
        assert_eq!(held(true, true), ["text/plain"]);
        assert_eq!(held(false, true), [super::PICTURE]);
        assert!(held(false, false).is_empty());
    }
}

/// macOS: the general pasteboard.
#[cfg(target_os = "macos")]
mod cocoa {
    // `class()` is the trait's, not the type's: a class object is what
    // `readObjectsForClasses` is asking for.
    use objc2::ClassType;
    use objc2_app_kit::{
        NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
    };
    use objc2_foundation::{NSArray, NSData, NSString, NSURL};

    /// What is on it, said in mime types.
    ///
    /// Only the three Obelus asks about. The pasteboard names dozens of
    /// shapes -- a copied file carries its name, its icon and its
    /// promises -- and reporting all of them would be reporting a
    /// vocabulary nothing here speaks.
    pub(crate) fn types() -> Option<Vec<String>> {
        let held = super::Holding {
            files: !urls().is_empty(),
            words: string().is_some(),
            picture: picture_type().is_some(),
        };
        Some(held.names())
    }

    /// One shape, as bytes.
    pub(crate) fn paste_as(mime: &str) -> Option<Vec<u8>> {
        if mime == super::super::FILES {
            let list: Vec<String> = urls().into_iter().map(|url| url.to_string()).collect();
            return (!list.is_empty()).then(|| list.join("\r\n").into_bytes());
        }
        if mime == super::PICTURE {
            return picture();
        }
        if mime.starts_with("text/") {
            return string().map(String::into_bytes);
        }
        None
    }

    /// Which of the two shapes a picture goes by is there, without reading
    /// it: a screenshot is a megabyte, and this is asked on every paste.
    fn picture_type() -> Option<objc2::rc::Retained<NSString>> {
        // Safety: as below; the two names are statics AppKit defines.
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            let wanted = NSArray::from_slice(&[NSPasteboardTypePNG, NSPasteboardTypeTIFF]);
            pasteboard.availableTypeFromArray(&wanted)
        }
    }

    /// The picture on it, as a PNG.
    ///
    /// A TIFF is what a copy out of most of macOS's own programs is, and
    /// it is made a PNG here rather than sent on: an agent is not obliged
    /// to read one.
    fn picture() -> Option<Vec<u8>> {
        // Safety: the general pasteboard is a singleton that exists for
        // the life of the process, and the data is copied out before it
        // is dropped.
        let (png, tiff) = unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            (
                pasteboard.dataForType(NSPasteboardTypePNG),
                pasteboard.dataForType(NSPasteboardTypeTIFF),
            )
        };
        if let Some(png) = png.map(|png| png.to_vec())
            && !png.is_empty()
        {
            return Some(png);
        }
        let tiff = tiff?.to_vec();
        match image::load_from_memory_with_format(&tiff, image::ImageFormat::Tiff) {
            Ok(picture) => super::as_png(&picture),
            Err(error) => {
                tracing::warn!(%error, "reading a TIFF off the pasteboard");
                None
            }
        }
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

    /// Puts a copy on the pasteboard, in every shape it was made in.
    ///
    /// A pasteboard names its shapes with UTIs, and a mime type is a
    /// perfectly good one: anything that is not a standard type is a
    /// custom type, named by whatever string it is given. So Obelus's own
    /// shape goes on under its own name and another Obelus asks for it by
    /// that name, with nothing in between -- which is the whole of what
    /// owning a selection buys on the other platform.
    pub(crate) fn copy(shapes: &[(String, Vec<u8>)]) -> bool {
        // Safety: the general pasteboard is a singleton that exists for
        // the life of the process, and every object below is made, handed
        // over and dropped inside this function.
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            // Everything that was on it before is somebody else's copy.
            // A pasteboard that was not cleared would answer for a shape
            // of the *previous* copy that this one does not have.
            pasteboard.clearContents();
            let mut written = false;
            let mut said_the_words = false;
            for (mime, bytes) in shapes {
                let data = NSData::with_bytes(bytes);
                let put = if super::is_words(mime) {
                    if said_the_words {
                        continue;
                    }
                    said_the_words = true;
                    pasteboard.setData_forType(Some(&data), NSPasteboardTypeString)
                } else {
                    pasteboard.setData_forType(Some(&data), &NSString::from_str(mime))
                };
                written |= put;
            }
            written
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
        // `GlobalFree` lives here rather than beside the rest of the
        // memory calls, which is windows-sys's arrangement and not one
        // that means anything.
        Foundation::{GlobalFree, HANDLE, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::{CF_DIB, CF_UNICODETEXT},
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
        // Safety: asking whether a format is there reads nothing and
        // cannot fail in a way that matters.
        let there = |format: u32| unsafe { IsClipboardFormatAvailable(format) } != 0;
        let held = super::Holding {
            files: there(CF_HDROP),
            words: there(CF_UNICODETEXT as u32),
            picture: there(png()) || there(CF_DIB as u32),
        };
        Some(held.names())
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
        if mime == super::PICTURE {
            return picture();
        }
        if mime.starts_with("text/") {
            return text().map(String::into_bytes);
        }
        None
    }

    /// The number the clipboard gives the format a PNG goes by.
    ///
    /// Registered by name rather than standard, because Windows has no PNG
    /// of its own: a browser and Office put one down under this name beside
    /// the DIB, and a zero -- the name could not be registered -- is a
    /// format nothing is ever available in.
    fn png() -> u32 {
        let name: Vec<u16> = "PNG".encode_utf16().chain(std::iter::once(0)).collect();
        // Safety: a terminated string, and registering one twice returns
        // the number the first call got.
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    }

    /// The picture on it, as a PNG.
    ///
    /// The PNG where one was put down, and otherwise the DIB made one --
    /// which is the only shape a screenshot taken with the system's own
    /// tool is in, and the one every picture is, because Windows makes it
    /// out of any other bitmap that is copied.
    fn picture() -> Option<Vec<u8>> {
        if let Some(png) = held(png())
            && !png.is_empty()
        {
            return Some(png);
        }
        png_of_dib(&held(CF_DIB as u32)?)
    }

    /// A DIB -- a bitmap file without the file's own header, which is how
    /// the clipboard keeps one -- as a PNG.
    fn png_of_dib(dib: &[u8]) -> Option<Vec<u8>> {
        let decoder =
            image::codecs::bmp::BmpDecoder::new_without_file_header(std::io::Cursor::new(dib));
        match decoder.and_then(image::DynamicImage::from_decoder) {
            Ok(picture) => super::as_png(&picture),
            Err(error) => {
                tracing::warn!(%error, "reading a DIB off the clipboard");
                None
            }
        }
    }

    /// One format's bytes, as they are on the clipboard.
    ///
    /// Only reached with the clipboard open.
    fn held(format: u32) -> Option<Vec<u8>> {
        // Safety: the handle belongs to the clipboard and is only read
        // while it is open and locked, for as many bytes as the block says
        // it holds.
        unsafe {
            let handle: HANDLE = GetClipboardData(format);
            if handle.is_null() {
                return None;
            }
            let locked = GlobalLock(handle.cast()).cast::<u8>();
            if locked.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(locked, GlobalSize(handle.cast())).to_vec();
            GlobalUnlock(handle.cast());
            Some(bytes)
        }
    }

    /// Puts a copy on the clipboard, in every shape it was made in.
    ///
    /// A format is a number here rather than a name, and
    /// `RegisterClipboardFormatW` is how a name becomes one: two programs
    /// that register the same string get the same number, which is how
    /// Obelus's own shape reaches another Obelus. The standard ones have
    /// their numbers already, and the words go under theirs because that
    /// is the one every other program on the machine looks for.
    pub(crate) fn copy(shapes: &[(String, Vec<u8>)]) -> bool {
        let Some(_open) = Open::taken() else {
            return false;
        };
        // Safety: only reached with the clipboard open, which is what
        // emptying it requires. Everything that was on it was somebody
        // else's copy, and a clipboard that was not emptied would answer
        // for a shape of the previous copy that this one does not have.
        if unsafe { EmptyClipboard() } == 0 {
            return false;
        }
        let mut written = false;
        let mut said_the_words = false;
        for (mime, bytes) in shapes {
            let (format, bytes) = if super::is_words(mime) {
                if said_the_words {
                    continue;
                }
                said_the_words = true;
                let Ok(said) = std::str::from_utf8(bytes) else {
                    continue;
                };
                // Terminated, because `CF_UNICODETEXT` is a C string: a
                // buffer without the zero is read past its end.
                let wide: Vec<u16> = crlf(said)
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                (CF_UNICODETEXT as u32, as_bytes(&wide))
            } else {
                // Safety: a name is a string the caller chose, and
                // registering one twice returns the number the first call
                // got.
                let name: Vec<u16> = mime.encode_utf16().chain(std::iter::once(0)).collect();
                let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
                if format == 0 {
                    continue;
                }
                (format, bytes.clone())
            };
            written |= put(format, &bytes);
        }
        written
    }

    /// Hands one shape to the clipboard, which owns the memory afterwards.
    ///
    /// Moveable memory, because that is what `SetClipboardData` documents
    /// it will free -- and it frees it, so a successful call must not free
    /// it as well and a failed one must.
    fn put(format: u32, bytes: &[u8]) -> bool {
        // Safety: the block is allocated here, filled here, and either
        // handed over or freed here.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if handle.is_null() {
                return false;
            }
            let locked = GlobalLock(handle);
            if locked.is_null() {
                GlobalFree(handle);
                return false;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
            GlobalUnlock(handle);
            if SetClipboardData(format, handle.cast::<std::ffi::c_void>()).is_null() {
                GlobalFree(handle);
                return false;
            }
            true
        }
    }

    /// Words with their line breaks as Windows writes them.
    ///
    /// What `win32yank -i --crlf` did when it was how Obelus wrote this
    /// clipboard: a program that reads `CF_UNICODETEXT` as Windows text
    /// pastes a bare `\n` as no break at all, so every line of a copy
    /// arrives joined to the next. A `\r\n` already there is left as one.
    fn crlf(said: &str) -> String {
        said.replace("\r\n", "\n").replace('\n', "\r\n")
    }

    /// The bytes of a UTF-16 buffer, in this machine's own order.
    ///
    /// Which is the order the clipboard wants: `CF_UNICODETEXT` is
    /// whatever `wchar_t` is on the machine reading it, and that is the
    /// machine that wrote it.
    fn as_bytes(wide: &[u16]) -> Vec<u8> {
        wide.iter().flat_map(|unit| unit.to_ne_bytes()).collect()
    }

    /// The text on the clipboard, with its line breaks as `\n`.
    ///
    /// Every Windows program puts `\r\n` there, Obelus included, and a
    /// buffer handed that gets a `\r` at the end of every line it did not
    /// have -- which is why `win32yank` was asked with `--lf` when it was
    /// the only way Obelus read this clipboard.
    fn text() -> Option<String> {
        written().map(|said| said.replace("\r\n", "\n"))
    }

    /// The same, as it is written there: UTF-16 turned into a string.
    fn written() -> Option<String> {
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

    #[cfg(test)]
    mod tests {
        use image::GenericImageView;

        /// The header a DIB starts with: `BITMAPINFOHEADER`, two pixels
        /// wide and one high, in `bits` to a pixel.
        fn header(bits: u16, compression: u32) -> Vec<u8> {
            let mut header = Vec::new();
            header.extend(40u32.to_le_bytes());
            header.extend(2i32.to_le_bytes());
            header.extend(1i32.to_le_bytes());
            header.extend(1u16.to_le_bytes());
            header.extend(bits.to_le_bytes());
            header.extend(compression.to_le_bytes());
            // The image's size, the two resolutions, and the two counts of
            // a palette nothing here has.
            header.extend([0; 20]);
            header
        }

        /// The colours of a PNG's two pixels.
        fn pixels(png: &[u8]) -> [[u8; 3]; 2] {
            let picture = image::load_from_memory(png).expect("a PNG Obelus can read");
            let rgb = |x| {
                let [r, g, b, _] = picture.get_pixel(x, 0).0;
                [r, g, b]
            };
            [rgb(0), rgb(1)]
        }

        /// A DIB is made a PNG of the same picture.
        ///
        /// Deliberate break: `BmpDecoder::new` in `png_of_dib`, which looks
        /// for the file header a DIB on the clipboard has not got, and
        /// makes nothing of it.
        #[test]
        fn a_dib_is_made_a_png() {
            let mut dib = header(24, 0);
            // Blue, green, red: a red pixel and a blue one, and the row
            // made up to four bytes.
            dib.extend([0, 0, 255, 255, 0, 0, 0, 0]);
            let png = super::png_of_dib(&dib).expect("a PNG");
            assert_eq!(pixels(&png), [[255, 0, 0], [0, 0, 255]]);
        }

        /// And so is the shape Windows itself makes one in, out of a
        /// screenshot: thirty-two bits with the three masks after the
        /// header, which say where each colour is in a pixel.
        ///
        /// Deliberate break: the masks left out of the DIB, which reads
        /// the pixels as masks and the picture as nothing.
        #[test]
        fn a_dib_with_masks_is_made_a_png() {
            let mut dib = header(32, 3);
            for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff] {
                dib.extend(mask.to_le_bytes());
            }
            dib.extend([0, 0, 255, 0, 255, 0, 0, 0]);
            let png = super::png_of_dib(&dib).expect("a PNG");
            assert_eq!(pixels(&png), [[255, 0, 0], [0, 0, 255]]);
        }

        /// A copy's line breaks are made Windows', and one already made so
        /// is not made so twice.
        ///
        /// Deliberate break: `crlf` as a bare `replace('\n', "\r\n")`,
        /// which writes `\r\r\n` for the break that was already there.
        #[test]
        fn a_line_break_is_made_a_windows_one_once() {
            assert_eq!(super::crlf("one\ntwo\r\nthree"), "one\r\ntwo\r\nthree");
        }

        /// And they are what lands on the clipboard, which is where another
        /// program reads them.
        ///
        /// Deliberate break: `copy` encoding `said` rather than
        /// `crlf(said)`, which leaves the bare `\n` there.
        #[test]
        #[ignore = "writes the clipboard of whoever runs it"]
        fn a_copy_lands_with_windows_line_breaks() {
            let shapes = [("text/plain".to_string(), b"one\ntwo\n".to_vec())];
            assert!(super::copy(&shapes), "the clipboard did not take the copy");
            let _open = super::Open::taken().expect("opening the clipboard");
            assert_eq!(super::written().as_deref(), Some("one\r\ntwo\r\n"));
        }
    }
}
