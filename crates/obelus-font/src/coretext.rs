//! The glyphs of the faces only CoreText can draw, drawn by CoreText.
//!
//! **A face only CoreText can draw is drawn by CoreText, in the face CoreText
//! itself would choose.**
//!
//! Every glyph is drawn by swash, which reads outlines from `glyf`, `CFF `
//! and `CFF2` and pictures from the tables beside them. `PingFang SC` has
//! none of those: its outlines are in `hvgl`, a table of Apple's that only
//! CoreText reads -- and it is the face macOS draws Chinese in, the one a
//! reader on a Mac sees everywhere else. So a face swash cannot draw and
//! CoreText can is kept, and its glyphs are drawn here instead (see
//! [`crate::faces`]).
//!
//! Only the drawing. What the face is called, what is in its character map
//! and how far each glyph advances are ordinary tables, and they are read
//! the way every other face's are, which is why a glyph number cosmic-text
//! shaped is the number CoreText is asked for: it is the same file.
//!
//! And in the face CoreText itself would draw. `PingFangUI.ttc` holds one
//! face a region, `PingFang SC Medium`, and every other weight is a place on
//! its `wght` axis -- so the face the font database knows is Medium, and a
//! line of Chinese drawn in it stood out from the Menlo beside it as bold.
//! What macOS draws after Menlo is `PingFangSC-Regular`, and after Menlo
//! Bold `PingFangSC-Semibold`: its own cascade list says so, by name, and
//! that is the name each weight is drawn in.
//!
//! In one shade, the way swash draws a letter: the coverage, with the colour
//! coming from the cell. Without font smoothing, which is CoreText thickening
//! the strokes of light text on a dark page -- a letter from here beside one
//! from swash would be the heavier of the two.

use std::collections::HashMap;

use cosmic_text::{CacheKey, SwashImage, fontdb};

/// What a face only CoreText draws is drawn in, by PostScript name: for
/// text, and for bold text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Names {
    /// For text at an ordinary weight.
    pub(crate) plain: String,
    /// For text at a bold one.
    pub(crate) bold: String,
}

impl Names {
    /// The one a glyph at this weight is drawn in.
    pub(crate) fn at(&self, weight: fontdb::Weight) -> &str {
        match weight.0 >= fontdb::Weight::SEMIBOLD.0 {
            true => &self.bold,
            false => &self.plain,
        }
    }
}

/// What each face CoreText draws is drawn in: the face of its family that
/// CoreText falls back to after `monospace`, plainly and in bold, where its
/// cascade list names that family -- and the face's own name where not.
pub(crate) fn names(
    db: &fontdb::Database,
    own: HashMap<fontdb::ID, String>,
    fallback: &Fallback,
) -> HashMap<fontdb::ID, Names> {
    named(db, own, &fallback.chosen(), file_of)
}

/// The same, from what CoreText chose and where it keeps each face.
///
/// A face of the family is taken only from the file the font database read
/// the family from. A glyph is asked for by the number cosmic-text shaped,
/// and that number is the file's: another file's face of the same name --
/// one the reader installed over the system's -- would draw a different
/// glyph at it.
fn named(
    db: &fontdb::Database,
    own: HashMap<fontdb::ID, String>,
    chosen: &HashMap<(String, bool), String>,
    file_of: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> HashMap<fontdb::ID, Names> {
    own.into_iter()
        .map(|(id, name)| {
            let face = db.face(id);
            let family = face
                .and_then(|face| face.families.first())
                .map(|(family, _)| family.to_lowercase())
                .unwrap_or_default();
            let path = face.and_then(|face| match &face.source {
                fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => Some(path),
                fontdb::Source::Binary(_) => None,
            });
            let same_file = |chosen: &&String| path.is_some() && file_of(chosen).as_ref() == path;
            let plain = chosen
                .get(&(family.clone(), false))
                .filter(same_file)
                .cloned()
                .unwrap_or(name);
            let bold = chosen
                .get(&(family, true))
                .filter(same_file)
                .cloned()
                .unwrap_or_else(|| plain.clone());
            (id, Names { plain, bold })
        })
        .collect()
}

/// The file CoreText draws the face by this PostScript name from.
#[cfg(target_os = "macos")]
fn file_of(name: &str) -> Option<std::path::PathBuf> {
    use objc2_core_foundation::{CFString, CFURL};
    use objc2_core_text::{CTFont, kCTFontURLAttribute};

    // SAFETY: as in `draws`.
    let font = unsafe { CTFont::with_name(&CFString::from_str(name), 0.0, std::ptr::null()) };
    // SAFETY: a static CoreText exports, asked of a font by its own key.
    let url = unsafe { font.attribute(kCTFontURLAttribute) }?;
    url.downcast_ref::<CFURL>()?.to_file_path()
}

/// Nowhere else is there a CoreText to ask.
#[cfg(not(target_os = "macos"))]
const fn file_of(_: &str) -> Option<std::path::PathBuf> {
    None
}

/// What CoreText falls back to after the monospaced face, plainly and in
/// bold: each family, and the face of it CoreText would draw in. Asked once,
/// and read twice -- for the order a character is tried in, and for which
/// face of a family it is drawn in.
#[derive(Debug, Default)]
pub(crate) struct Fallback {
    plain: Vec<(String, String)>,
    bold: Vec<(String, String)>,
}

impl Fallback {
    /// What CoreText falls back to after this face, where there is a
    /// CoreText to ask.
    pub(crate) fn after(monospace: Option<&str>) -> Self {
        monospace.map_or_else(Self::default, |monospace| Self {
            plain: cascade(monospace, false),
            bold: cascade(monospace, true),
        })
    }

    /// The families, in CoreText's order.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(dead_code, reason = "only CoreText's")
    )]
    pub(crate) fn families(&self) -> Vec<String> {
        self.plain
            .iter()
            .map(|(family, _)| family.clone())
            .collect()
    }

    /// The face of each family, by the family's name in lower case and
    /// whether it is bold.
    fn chosen(&self) -> HashMap<(String, bool), String> {
        let mut chosen = HashMap::new();
        for (bold, list) in [(false, &self.plain), (true, &self.bold)] {
            for (family, name) in list {
                chosen
                    .entry((family.to_lowercase(), bold))
                    .or_insert_with(|| name.clone());
            }
        }
        chosen
    }
}

/// What CoreText falls back to after the face by this family name, plainly
/// or in bold, in the reader's languages and in its order: each family, and
/// the face of it CoreText would draw in.
#[cfg(target_os = "macos")]
fn cascade(monospace: &str, bold: bool) -> Vec<(String, String)> {
    use objc2_core_foundation::{CFArray, CFLocale, CFRetained, CFString};
    use objc2_core_text::{
        CTFont, CTFontDescriptor, CTFontSymbolicTraits, kCTFontFamilyNameAttribute,
        kCTFontNameAttribute,
    };

    // SAFETY: a size of nothing is the face's own size, and no matrix is
    // the identity.
    let font = unsafe { CTFont::with_name(&CFString::from_str(monospace), 0.0, std::ptr::null()) };
    let font = match bold {
        // SAFETY: as above, and bold asked for under a mask of bold alone.
        true => unsafe {
            font.copy_with_symbolic_traits(
                0.0,
                std::ptr::null(),
                CTFontSymbolicTraits::TraitBold,
                CTFontSymbolicTraits::TraitBold,
            )
        }
        .unwrap_or(font),
        false => font,
    };
    let languages = CFLocale::preferred_languages();
    // SAFETY: the languages are an array of strings, which is what it asks.
    let Some(list) = (unsafe { font.default_cascade_list_for_languages(languages.as_deref()) })
    else {
        return Vec::new();
    };
    // SAFETY: an array of descriptors is what it is documented to return.
    let list = unsafe { CFRetained::cast_unchecked::<CFArray<CTFontDescriptor>>(list) };
    let text = |descriptor: &CTFontDescriptor, key: &CFString| {
        // SAFETY: an attribute asked by its own key.
        let said = unsafe { descriptor.attribute(key) }?;
        said.downcast_ref::<CFString>().map(ToString::to_string)
    };
    list.iter()
        .filter_map(|descriptor| {
            // SAFETY: statics CoreText exports.
            let (family, name) = unsafe { (kCTFontFamilyNameAttribute, kCTFontNameAttribute) };
            Some((text(&descriptor, family)?, text(&descriptor, name)?))
        })
        .collect()
}

/// Nor a cascade to ask.
#[cfg(not(target_os = "macos"))]
const fn cascade(_: &str, _: bool) -> Vec<(String, String)> {
    Vec::new()
}

/// Whether CoreText draws the face by this PostScript name.
///
/// Asked by making the font and seeing whether it is that face: CoreText
/// answers a name it does not know with some other face rather than with
/// nothing. Not asked at all of a name starting with a dot, which is one of
/// the system's own: CoreText will not hand one out by name, and says so
/// on stderr every time it is asked.
#[cfg(target_os = "macos")]
pub(crate) fn draws(name: &str) -> bool {
    use objc2_core_foundation::CFString;
    use objc2_core_text::CTFont;

    if name.starts_with('.') {
        return false;
    }

    // SAFETY: a size of nothing is the face's own size, and no matrix is the
    // identity.
    let font = unsafe { CTFont::with_name(&CFString::from_str(name), 0.0, std::ptr::null()) };
    // SAFETY: a font CoreText made always has a PostScript name.
    unsafe { font.post_script_name() }.to_string() == name
}

/// Nowhere else is there a CoreText to draw with.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn draws(_: &str) -> bool {
    false
}

/// One glyph of the face by this PostScript name, at the size and the
/// offset the key asks for -- or nothing, where it has no pixels, which is
/// a space.
#[cfg(target_os = "macos")]
pub(crate) fn draw(name: &str, key: CacheKey) -> Option<SwashImage> {
    use std::ptr::NonNull;

    use cosmic_text::{CacheKeyFlags, Placement, SwashContent};
    use objc2_core_foundation::{CFString, CGAffineTransform, CGFloat, CGPoint, CGRect};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGGlyph, CGImageAlphaInfo,
    };
    use objc2_core_text::{CTFont, CTFontOrientation};

    let size = CGFloat::from(f32::from_bits(key.font_size_bits));
    // Slanted where the text is italic and the face has no italic of its
    // own, by the fourteen degrees swash slants the glyphs beside it. In the
    // font's own matrix rather than the context's, so that the box asked
    // for below is the slanted glyph's.
    let slant = match key.flags.contains(CacheKeyFlags::FAKE_ITALIC) {
        true => 14.0_f64.to_radians().tan(),
        false => 0.0,
    };
    let matrix = CGAffineTransform {
        a: 1.0,
        b: 0.0,
        c: slant,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };
    // SAFETY: as in `draws`, with a matrix that outlives the call.
    let font = unsafe { CTFont::with_name(&CFString::from_str(name), size, &raw const matrix) };
    let glyph: CGGlyph = key.glyph_id;
    let mut bounds = CGRect::default();
    // SAFETY: one glyph, and room for its one rectangle.
    unsafe {
        font.bounding_rects_for_glyphs(
            CTFontOrientation::Default,
            NonNull::from(&glyph),
            &raw mut bounds,
            1,
        );
    };
    if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return None;
    }
    // Where the pen is inside its pixel, which is the part of the position
    // the grid could not put it at.
    let offset = CGFloat::from(key.x_bin.as_float());
    // A pixel of room each way, for what antialiasing spills past the
    // outline's own box.
    let left = (bounds.origin.x + offset).floor() - 1.0;
    let right = (bounds.origin.x + bounds.size.width + offset).ceil() + 1.0;
    let bottom = bounds.origin.y.floor() - 1.0;
    let top = (bounds.origin.y + bounds.size.height).ceil() + 1.0;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a glyph is a few dozen whole pixels across"
    )]
    let (width, height) = ((right - left) as usize, (top - bottom) as usize);
    let mut data = vec![0_u8; width * height];
    let gray = CGColorSpace::new_device_gray();
    // SAFETY: `data` is a row of `width` bytes for each of `height` rows, and
    // outlives the context, which is dropped before it is read.
    let context = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr().cast(),
            width,
            height,
            8,
            width,
            gray.as_deref(),
            CGImageAlphaInfo::None.0,
        )
    }?;
    CGContext::set_gray_fill_color(Some(&context), 1.0, 1.0);
    CGContext::set_should_antialias(Some(&context), true);
    CGContext::set_allows_font_smoothing(Some(&context), false);
    CGContext::set_should_smooth_fonts(Some(&context), false);
    // Drawn where `offset` says inside its pixel rather than rounded to the
    // nearest whole one, which is what the key's offset is for.
    CGContext::set_allows_font_subpixel_positioning(Some(&context), true);
    CGContext::set_should_subpixel_position_fonts(Some(&context), true);
    // CoreGraphics counts up from the bottom of the bitmap; its first row in
    // memory is the top, which is the row swash puts first too.
    let at = CGPoint {
        x: offset - left,
        y: -bottom,
    };
    // SAFETY: one glyph at one position, into a context that is alive.
    unsafe { font.draw_glyphs(NonNull::from(&glyph), NonNull::from(&at), 1, &context) };
    drop(context);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a glyph is a few dozen whole pixels from the pen"
    )]
    let placement = Placement {
        left: left as i32,
        top: top as i32,
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
    };
    let mut image = SwashImage::new();
    image.content = SwashContent::Mask;
    image.placement = placement;
    image.data = data;
    Some(image)
}

/// Nor anything to draw.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn draw(_: &str, _: CacheKey) -> Option<SwashImage> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The face CoreText chose is drawn in where it is in the file the
    /// family was read from, and the face's own name where it is not.
    ///
    /// Deliberate break: not asking which file -- `same_file` answering yes
    /// -- takes the name from the other file, whose glyph numbers are not
    /// the ones that were shaped.
    #[test]
    fn a_chosen_face_is_drawn_in_only_from_the_same_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fonts/SymbolsNerdFontMono-Regular.ttf");
        let mut db = fontdb::Database::new();
        db.load_font_file(&path).expect("the face Obelus carries");
        let face = db.faces().next().expect("one face");
        let (id, family) = (face.id, face.families[0].0.to_lowercase());
        let own = HashMap::from([(id, "Own".to_string())]);
        let chosen = HashMap::from([
            ((family.clone(), false), "Chosen".to_string()),
            ((family, true), "Chosen-Bold".to_string()),
        ]);

        let here = named(&db, own.clone(), &chosen, |_| Some(path.clone()));
        assert_eq!(
            here[&id],
            Names {
                plain: "Chosen".to_string(),
                bold: "Chosen-Bold".to_string()
            }
        );

        let elsewhere = named(&db, own, &chosen, |_| Some("/somewhere/else.ttc".into()));
        assert_eq!(
            elsewhere[&id],
            Names {
                plain: "Own".to_string(),
                bold: "Own".to_string()
            }
        );
    }
}
