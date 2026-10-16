//! Which encoding a file's bytes are written in, and writing them back the
//! same way.
//!
//! Everything inside Obelus is UTF-8 -- a rope, a line, a query -- so a file
//! written in anything else is decoded on the way in and encoded on the way
//! out, the way Helix does it. One function says what a file's bytes are,
//! and everything that reads a project's files asks it: the buffer, the
//! search, git's version of the same file. Two answers would be a search
//! that finds a line on a row the editor numbers differently, or a margin
//! that marks every line of a file changed because the version in `HEAD`
//! was read in another encoding.
//!
//! **What Obelus reads, it can write back byte for byte.** A guess is a
//! guess, and the cost of a wrong one must be a page of mojibake the reader
//! can see and put right -- never a file that a save quietly rewrote. So a
//! file is text only if decoding it raised no error *and* encoding what came
//! out gives back exactly the bytes that went in; anything else is not text,
//! which is what it was before any of this existed. Helix shipped without
//! the second half, and files grew diffs the moment they were saved.

use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

/// How much of a file is looked at for a NUL before deciding it is not
/// text. The same window `git` and ripgrep look through.
const SNIFFED: usize = 8 * 1024;

/// How a file's bytes are written: in which encoding, and whether they start
/// with the mark that says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Written {
    encoding: &'static Encoding,
    /// Whether the file starts with a byte order mark. Kept apart from the
    /// text -- a U+FEFF at the front of the rope was a character the reader
    /// could delete and a column every position on the first line was out
    /// by -- and written back in front of it.
    bom: bool,
}

/// A character the encoding a file is written in has no bytes for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unwritable {
    /// The first of them.
    pub character: char,
    /// The encoding that could not write it.
    pub written: Written,
}

impl std::fmt::Display for Unwritable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} has no way to write {:?} (U+{:04X})",
            self.written.name(),
            self.character,
            u32::from(self.character)
        )
    }
}

impl std::error::Error for Unwritable {}

/// What a file's bytes say, and how they were written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read<'a> {
    /// The text, in UTF-8, without the byte order mark.
    pub text: Cow<'a, str>,
    /// How to write it back.
    pub written: Written,
}

impl Read<'_> {
    /// The same, owning its text.
    #[must_use]
    pub fn into_owned(self) -> Read<'static> {
        Read {
            text: Cow::Owned(self.text.into_owned()),
            written: self.written,
        }
    }
}

impl Default for Written {
    fn default() -> Self {
        Self::UTF8
    }
}

impl Written {
    /// UTF-8 with nothing in front of it: what almost every file is, and
    /// what a new one is written in.
    pub const UTF8: Self = Self {
        encoding: UTF_8,
        bom: false,
    };

    /// The encodings a reader is offered when the guess was wrong, in the
    /// order they are offered.
    ///
    /// Every encoding `encoding_rs` can read and write, which is the
    /// Encoding Standard's list: Unicode first, then East Asia's, then the
    /// single-byte code pages by the script they are for.
    ///
    /// All of them and not a few chosen, because the guess can name any of
    /// them, and a list that did not have the one it named could not open on
    /// it. Left out are the two the standard keeps for its own reasons:
    /// `replacement`, which decodes everything to one U+FFFD, and
    /// `x-user-defined`, which is a way of not decoding at all.
    #[must_use]
    pub fn offered() -> Vec<Self> {
        use encoding_rs::*;
        let plain = |encoding| Self {
            encoding,
            bom: false,
        };
        let marked = |encoding| Self {
            encoding,
            bom: true,
        };
        vec![
            plain(UTF_8),
            marked(UTF_8),
            marked(UTF_16LE),
            marked(UTF_16BE),
            // Chinese, Japanese, Korean.
            plain(GBK),
            plain(GB18030),
            plain(BIG5),
            plain(SHIFT_JIS),
            plain(EUC_JP),
            plain(ISO_2022_JP),
            plain(EUC_KR),
            // Western European.
            plain(WINDOWS_1252),
            plain(ISO_8859_15),
            plain(MACINTOSH),
            // Central and Eastern European, Baltic, Celtic, Nordic.
            plain(WINDOWS_1250),
            plain(ISO_8859_2),
            plain(ISO_8859_16),
            plain(WINDOWS_1257),
            plain(ISO_8859_4),
            plain(ISO_8859_13),
            plain(ISO_8859_10),
            plain(ISO_8859_14),
            plain(ISO_8859_3),
            // Cyrillic.
            plain(WINDOWS_1251),
            plain(KOI8_R),
            plain(KOI8_U),
            plain(ISO_8859_5),
            plain(IBM866),
            plain(X_MAC_CYRILLIC),
            // Greek, Turkish, Hebrew, Arabic, Thai, Vietnamese.
            plain(WINDOWS_1253),
            plain(ISO_8859_7),
            plain(WINDOWS_1254),
            plain(WINDOWS_1255),
            plain(ISO_8859_8),
            plain(ISO_8859_8_I),
            plain(WINDOWS_1256),
            plain(ISO_8859_6),
            plain(WINDOWS_874),
            plain(WINDOWS_1258),
        ]
    }

    /// Its name, as the encoding is named everywhere else -- `GBK`,
    /// `Shift_JIS`, `UTF-16LE` -- and whether it carries the mark.
    #[must_use]
    pub fn name(self) -> Cow<'static, str> {
        match self.bom && self.encoding == UTF_8 {
            true => Cow::Borrowed("UTF-8 with BOM"),
            false => Cow::Borrowed(self.encoding.name()),
        }
    }

    /// Whether this is UTF-8 with nothing in front of it, which is the one a
    /// reader is never told about.
    #[must_use]
    pub fn is_plain(self) -> bool {
        self == Self::UTF8
    }

    /// Whether the bytes start with a byte order mark, which a save puts in
    /// front of the text.
    #[must_use]
    pub const fn is_marked(self) -> bool {
        self.bom
    }

    /// Whether a line of the text is a line of the bytes: whether a `\n` in
    /// one is a `\n` in the other.
    ///
    /// Not in UTF-16, where a newline is two bytes and one of them is a NUL.
    /// What reads a file's bytes a line at a time without decoding them --
    /// git's blame -- has lines that are not the reader's.
    #[must_use]
    pub fn lines_are_bytes(self) -> bool {
        self.encoding != UTF_16LE && self.encoding != UTF_16BE
    }

    /// Reads bytes as this encoding rather than as whatever they look like:
    /// what a reader who has said which encoding a file is in gets.
    ///
    /// A byte order mark for this encoding is taken off the front and
    /// written back on the way out, whether or not this says it has one;
    /// one for another encoding is a mark this encoding cannot read.
    ///
    /// `None` where the bytes are not this encoding -- by the same rule
    /// [`read`] keeps, that what is read can be written back byte for byte.
    #[must_use]
    pub fn read(self, bytes: &[u8]) -> Option<Read<'_>> {
        let (body, bom) = match Encoding::for_bom(bytes) {
            Some((marked, length)) if marked == self.encoding => (&bytes[length..], true),
            _ => (bytes, false),
        };
        let written = Self {
            encoding: self.encoding,
            bom,
        };
        let text = self
            .encoding
            .decode_without_bom_handling_and_without_replacement(body)?;
        // UTF-8 that decoded without error is its own bytes. Everything else
        // is asked, because some decoders map two byte sequences onto one
        // character and an encoder can only give one of them back.
        if self.encoding != UTF_8 {
            match written.encode(&text) {
                Ok(again) if again == bytes => {}
                _ => return None,
            }
        }
        Some(Read { text, written })
    }

    /// The bytes that write `text` in this encoding, the mark in front where
    /// there is one.
    ///
    /// # Errors
    ///
    /// The first character this encoding has no bytes for. Not replaced:
    /// `encoding_rs` would write `&#128512;` in its place, which is a file
    /// that says something its reader did not write.
    pub fn encode(self, text: &str) -> Result<Cow<'_, [u8]>, Unwritable> {
        if self.encoding == UTF_8 {
            return Ok(match self.bom {
                false => Cow::Borrowed(text.as_bytes()),
                true => Cow::Owned([b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat()),
            });
        }
        // `encoding_rs` decodes UTF-16 and does not encode it -- the Web never
        // writes it -- so its output encoding for both is UTF-8.
        let wide = |unit: u16| match self.encoding == UTF_16LE {
            true => unit.to_le_bytes(),
            false => unit.to_be_bytes(),
        };
        if self.encoding == UTF_16LE || self.encoding == UTF_16BE {
            let mut out = Vec::with_capacity(text.len() * 2 + 2);
            if self.bom {
                out.extend(wide(0xFEFF));
            }
            for unit in text.encode_utf16() {
                out.extend(wide(unit));
            }
            return Ok(Cow::Owned(out));
        }
        let mut encoder = self.encoding.new_encoder();
        let room = encoder
            .max_buffer_length_from_utf8_without_replacement(text.len())
            .unwrap_or(text.len() * 4);
        let mut out = Vec::with_capacity(room);
        let (result, _) = encoder.encode_from_utf8_to_vec_without_replacement(text, &mut out, true);
        match result {
            encoding_rs::EncoderResult::InputEmpty => Ok(Cow::Owned(out)),
            encoding_rs::EncoderResult::Unmappable(character) => Err(Unwritable {
                character,
                written: self,
            }),
            // The room was asked of the encoder for exactly this text.
            encoding_rs::EncoderResult::OutputFull => unreachable!("an encoder ran out of room"),
        }
    }
}

/// What a file's bytes say, or `None` where they are not text.
///
/// In order: a byte order mark says which encoding; UTF-8 that is valid is
/// UTF-8, which is nearly every file and costs one pass; a NUL in the first
/// few kilobytes is a binary file; and anything else is guessed at by
/// `chardetng` -- GBK, Shift_JIS, a Windows code page -- and kept only if it
/// can be written back the way it came (the crate's doc says why).
#[must_use]
pub fn read(bytes: &[u8]) -> Option<Read<'_>> {
    if let Some((encoding, _)) = Encoding::for_bom(bytes) {
        return Written {
            encoding,
            bom: true,
        }
        .read(bytes);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(Read {
            text: Cow::Borrowed(text),
            written: Written::UTF8,
        });
    }
    if bytes[..bytes.len().min(SNIFFED)].contains(&0) {
        return None;
    }
    // Neither of the two the guess may be told to allow could be the
    // answer here: ISO-2022-JP is seven-bit and so valid UTF-8, and these
    // bytes have already been asked whether they are UTF-8.
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let encoding = detector.guess(None, chardetng::Utf8Detection::Deny);
    Written {
        encoding,
        bom: false,
    }
    .read(bytes)
}

/// Reads a file and says what it says.
///
/// # Errors
///
/// The file's own, and [`std::io::ErrorKind::InvalidData`] where its bytes
/// are not text -- the error `read_to_string` gave for every file that was not
/// UTF-8, so a caller that said "could not open it" still does.
pub fn read_file(path: &std::path::Path) -> std::io::Result<Read<'static>> {
    let bytes = std::fs::read(path)?;
    owned(bytes)
}

/// What bytes already read say, owning the text.
///
/// # Errors
///
/// [`std::io::ErrorKind::InvalidData`] where they are not text.
pub fn owned(bytes: Vec<u8>) -> std::io::Result<Read<'static>> {
    // UTF-8 keeps its allocation: the bytes are the text. Asked after the
    // mark, because UTF-8 with a BOM is valid UTF-8 with a U+FEFF in it.
    if Encoding::for_bom(&bytes).is_none() {
        let bytes = match String::from_utf8(bytes) {
            Ok(text) => {
                return Ok(Read {
                    text: Cow::Owned(text),
                    written: Written::UTF8,
                });
            }
            Err(error) => error.into_bytes(),
        };
        return read(&bytes).map(Read::into_owned).ok_or_else(not_text);
    }
    read(&bytes).map(Read::into_owned).ok_or_else(not_text)
}

fn not_text() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "not text in any encoding Obelus can write back",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_is_its_own_bytes() {
        let read = read("fn main() {}\n".as_bytes()).expect("text");
        assert!(matches!(read.text, Cow::Borrowed(_)), "UTF-8 was copied");
        assert!(read.written.is_plain());
    }

    #[test]
    fn a_bom_is_kept_apart_from_the_text_and_written_back() {
        let bytes = b"\xEF\xBB\xBFhello\n";
        let read = read(bytes).expect("text");
        assert_eq!(read.text, "hello\n");
        assert_eq!(read.written.name(), "UTF-8 with BOM");
        assert_eq!(
            read.written.encode(&read.text).expect("writable"),
            &bytes[..]
        );
    }

    #[test]
    fn utf16_is_read_and_written_back() {
        let bytes: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("你好\nbeta\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let read = read(&bytes).expect("text");
        assert_eq!(read.text, "你好\nbeta\n");
        assert_eq!(read.written.name(), "UTF-16LE");
        assert!(!read.written.lines_are_bytes());
        assert_eq!(read.written.encode(&read.text).expect("writable"), bytes);
    }

    #[test]
    fn gbk_is_guessed_and_written_back() {
        let (bytes, _, _) = encoding_rs::GBK
            .encode("// 这是一个用中文写的注释，说明这段代码做什么\nfn main() {}\n");
        let read = read(&bytes).expect("text");
        assert_eq!(
            read.text,
            "// 这是一个用中文写的注释，说明这段代码做什么\nfn main() {}\n"
        );
        // `chardetng` names the family by its Web label; GBK and gb18030
        // decode these bytes the same way.
        assert!(
            ["GBK", "gb18030"].contains(&read.written.encoding.name()),
            "guessed {}",
            read.written.name()
        );
        assert_eq!(read.written.encode(&read.text).expect("writable"), bytes);
    }

    #[test]
    fn a_nul_is_a_binary_file() {
        assert!(read(&[b'a', 0, 0xff, b'b']).is_none());
    }

    /// The bytes the tests across Obelus use for "a file that will not
    /// read": a UTF-16 mark with half a character after it, which decodes
    /// with an error and so is still not text.
    #[test]
    fn half_a_character_is_not_text() {
        assert!(read(&[0xff, 0xfe, 0xfd]).is_none());
    }

    /// Whatever the guess names is on the list, so the list can open on it
    /// and a reader can come back to it after trying another. Text in a
    /// dozen scripts, each in its own legacy code page.
    ///
    /// Broken by leaving `WINDOWS_1253` out of `offered`: the Greek is
    /// guessed as it and the list does not have it.
    #[test]
    fn whatever_is_guessed_is_offered() {
        let samples = [
            (
                encoding_rs::GBK,
                "这是一个用中文写的注释，说明这段代码做什么",
            ),
            (
                encoding_rs::BIG5,
                "這是一個用中文寫的註釋，說明這段程式碼做什麼",
            ),
            (
                encoding_rs::SHIFT_JIS,
                "これは日本語で書かれたコメントです、何をするか説明します",
            ),
            (
                encoding_rs::EUC_KR,
                "이것은 한국어로 작성된 주석입니다 무엇을 하는지 설명합니다",
            ),
            (
                encoding_rs::WINDOWS_1251,
                "Это комментарий, написанный по-русски, о том, что делает код",
            ),
            (
                encoding_rs::WINDOWS_1253,
                "Αυτό είναι ένα σχόλιο γραμμένο στα ελληνικά για τον κώδικα",
            ),
            (
                encoding_rs::WINDOWS_1255,
                "זוהי הערה שנכתבה בעברית ומסבירה מה הקוד עושה",
            ),
            (
                encoding_rs::WINDOWS_1256,
                "هذا تعليق مكتوب باللغة العربية يشرح ما يفعله الكود",
            ),
            (
                encoding_rs::WINDOWS_874,
                "นี่คือความคิดเห็นที่เขียนเป็นภาษาไทยเพื่ออธิบายโค้ด",
            ),
            (
                encoding_rs::WINDOWS_1254,
                "Bu, kodun ne yaptığını açıklayan Türkçe yazılmış bir yorumdur",
            ),
            (
                encoding_rs::WINDOWS_1250,
                "To jest komentarz napisany po polsku, wyjaśniający, co robi kod",
            ),
            (
                encoding_rs::WINDOWS_1252,
                "Ceci est un commentaire écrit en français, qui décrit le code",
            ),
        ];
        let offered = Written::offered();
        for (encoding, text) in samples {
            let (bytes, _, unmappable) = encoding.encode(text);
            assert!(
                !unmappable,
                "{} cannot write its own sample",
                encoding.name()
            );
            let guessed = read(&bytes)
                .unwrap_or_else(|| panic!("the {} sample is not text", encoding.name()))
                .written;
            assert!(
                offered.contains(&guessed),
                "{} was guessed and is not offered",
                guessed.name()
            );
        }
    }

    /// Shift_JIS has two byte pairs for some characters -- NEC's and IBM's
    /// -- and an encoder writes one of them. Read as text, `ED 40` would be
    /// saved as `FA 5C`. Broken by dropping the round trip from
    /// `Written::read`, which reads it.
    #[test]
    fn what_would_not_write_back_the_same_is_not_text() {
        let shift_jis = Written {
            encoding: encoding_rs::SHIFT_JIS,
            bom: false,
        };
        assert_eq!(shift_jis.encoding.decode(&[0xED, 0x40]).0, "\u{7E8A}");
        assert!(shift_jis.read(&[0xED, 0x40]).is_none());
    }

    #[test]
    fn what_gbk_cannot_write_is_refused_and_named() {
        let gbk = Written {
            encoding: encoding_rs::GBK,
            bom: false,
        };
        let refused = gbk.encode("ok 😀").expect_err("GBK has no emoji");
        assert_eq!(refused.character, '😀');
    }

    #[test]
    fn a_chosen_encoding_reads_what_the_guess_did_not() {
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("こんにちは\n");
        let shift_jis = Written {
            encoding: encoding_rs::SHIFT_JIS,
            bom: false,
        };
        assert_eq!(
            shift_jis.read(&bytes).expect("Shift_JIS").text,
            "こんにちは\n"
        );
        // And bytes that are not that encoding are not read as it.
        assert!(Written::UTF8.read(&bytes).is_none());
    }
}
