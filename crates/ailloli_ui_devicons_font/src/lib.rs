//! License-audited file-icon font embedded as raw bytes.
//!
//! The font is a deterministic subset of Nerd Fonts Symbols Only 3.4.0. It
//! contains only glyphs used by `devicons` 0.6.12 plus the canonical folder
//! and file fallbacks. See `SOURCE_PROVENANCE` and `THIRD_PARTY_NOTICES`.

/// Generic file glyph used whenever a requested icon is not in the audited font.
pub const GENERIC_FILE_GLYPH: char = '\u{f15b}';

/// Folder glyph used by the framework file explorer.
pub const FOLDER_GLYPH: char = '\u{f07b}';

/// Bytes of the audited file-icon font used for `IconId::Devicon`.
pub const DEVICON_FONT_BYTES: &[u8] = include_bytes!("../assets/AilloliUIFileIcons-Regular.ttf");

/// Glyphs retained by the audited subset, sorted by Unicode codepoint.
pub const SUPPORTED_GLYPHS: &[char] = &[
    '\u{e21c}',
    '\u{e271}',
    '\u{e28b}',
    '\u{e28c}',
    '\u{e600}',
    '\u{e603}',
    '\u{e606}',
    '\u{e607}',
    '\u{e608}',
    '\u{e609}',
    '\u{e60a}',
    '\u{e60b}',
    '\u{e60c}',
    '\u{e60d}',
    '\u{e60e}',
    '\u{e60f}',
    '\u{e610}',
    '\u{e611}',
    '\u{e614}',
    '\u{e615}',
    '\u{e619}',
    '\u{e61b}',
    '\u{e61c}',
    '\u{e61d}',
    '\u{e61e}',
    '\u{e61f}',
    '\u{e620}',
    '\u{e623}',
    '\u{e624}',
    '\u{e625}',
    '\u{e627}',
    '\u{e628}',
    '\u{e62b}',
    '\u{e62c}',
    '\u{e62d}',
    '\u{e62f}',
    '\u{e631}',
    '\u{e632}',
    '\u{e633}',
    '\u{e634}',
    '\u{e639}',
    '\u{e63a}',
    '\u{e63b}',
    '\u{e64a}',
    '\u{e64b}',
    '\u{e652}',
    '\u{e655}',
    '\u{e656}',
    '\u{e65f}',
    '\u{e660}',
    '\u{e666}',
    '\u{e667}',
    '\u{e670}',
    '\u{e672}',
    '\u{e674}',
    '\u{e677}',
    '\u{e67a}',
    '\u{e682}',
    '\u{e684}',
    '\u{e688}',
    '\u{e68b}',
    '\u{e691}',
    '\u{e697}',
    '\u{e69a}',
    '\u{e69b}',
    '\u{e69d}',
    '\u{e69e}',
    '\u{e69f}',
    '\u{e6a0}',
    '\u{e6a1}',
    '\u{e6a9}',
    '\u{e6ac}',
    '\u{e6af}',
    '\u{e6b2}',
    '\u{e6b3}',
    '\u{e6b4}',
    '\u{e702}',
    '\u{e706}',
    '\u{e707}',
    '\u{e70c}',
    '\u{e70e}',
    '\u{e718}',
    '\u{e71e}',
    '\u{e728}',
    '\u{e736}',
    '\u{e737}',
    '\u{e738}',
    '\u{e745}',
    '\u{e749}',
    '\u{e755}',
    '\u{e768}',
    '\u{e769}',
    '\u{e76a}',
    '\u{e772}',
    '\u{e775}',
    '\u{e779}',
    '\u{e786}',
    '\u{e791}',
    '\u{e795}',
    '\u{e798}',
    '\u{e7a1}',
    '\u{e7a7}',
    '\u{e7a8}',
    '\u{e7a9}',
    '\u{e7aa}',
    '\u{e7af}',
    '\u{e7b1}',
    '\u{e7b4}',
    '\u{e7b8}',
    '\u{e7ba}',
    '\u{eac4}',
    '\u{eae8}',
    '\u{eaeb}',
    '\u{eb9c}',
    '\u{ebc8}',
    '\u{ebe8}',
    '\u{f001}',
    '\u{f005}',
    '\u{f019}',
    '\u{f031}',
    '\u{f06d}',
    '\u{f073}',
    '\u{f076}',
    '\u{f07b}',
    '\u{f0ad}',
    '\u{f0c6}',
    '\u{f0ec}',
    '\u{f0fd}',
    '\u{f108}',
    '\u{f129}',
    '\u{f15b}',
    '\u{f179}',
    '\u{f17c}',
    '\u{f1ab}',
    '\u{f1b2}',
    '\u{f20e}',
    '\u{f23e}',
    '\u{f296}',
    '\u{f2b8}',
    '\u{f2d0}',
    '\u{f2f7}',
    '\u{f410}',
    '\u{f462}',
    '\u{f487}',
    '\u{f489}',
    '\u{f48a}',
    '\u{f499}',
    '\u{f49b}',
    '\u{f4ae}',
    '\u{f006f}',
    '\u{f00ab}',
    '\u{f019a}',
    '\u{f01a7}',
    '\u{f0219}',
    '\u{f021b}',
    '\u{f0227}',
    '\u{f022c}',
    '\u{f02a2}',
    '\u{f031b}',
    '\u{f032a}',
    '\u{f0331}',
    '\u{f035b}',
    '\u{f042b}',
    '\u{f0483}',
    '\u{f04d9}',
    '\u{f0509}',
    '\u{f0565}',
    '\u{f057c}',
    '\u{f05c0}',
    '\u{f05c6}',
    '\u{f05ca}',
    '\u{f0627}',
    '\u{f0673}',
    '\u{f06a9}',
    '\u{f06d3}',
    '\u{f0718}',
    '\u{f0721}',
    '\u{f072b}',
    '\u{f07d4}',
    '\u{f0858}',
    '\u{f0868}',
    '\u{f08c7}',
    '\u{f099d}',
    '\u{f0a0a}',
    '\u{f0a16}',
    '\u{f0aae}',
    '\u{f0bc4}',
    '\u{f0cb9}',
    '\u{f0dd6}',
    '\u{f0ebe}',
    '\u{f0eeb}',
    '\u{f1049}',
    '\u{f1106}',
    '\u{f121a}',
    '\u{f125f}',
    '\u{f13ff}',
    '\u{f1997}',
    '\u{f1998}',
];

/// Returns whether `glyph` is present in the audited embedded font.
pub fn supports_glyph(glyph: char) -> bool {
    SUPPORTED_GLYPHS.binary_search(&glyph).is_ok()
}

/// Returns `glyph` when it is audited and bundled, otherwise the generic file glyph.
pub fn glyph_or_fallback(glyph: char) -> char {
    if supports_glyph(glyph) {
        glyph
    } else {
        GENERIC_FILE_GLYPH
    }
}

#[cfg(test)]
mod tests {
    use super::{
        glyph_or_fallback, supports_glyph, DEVICON_FONT_BYTES, FOLDER_GLYPH, GENERIC_FILE_GLYPH,
        SUPPORTED_GLYPHS,
    };

    #[test]
    fn bundled_font_contains_exactly_the_audited_glyphs() {
        let face = ttf_parser::Face::parse(DEVICON_FONT_BYTES, 0).expect("parse icon font");
        assert_eq!(SUPPORTED_GLYPHS.len(), 198);
        assert!(SUPPORTED_GLYPHS.windows(2).all(|pair| pair[0] < pair[1]));
        for (label, ch) in [
            ("rust", '\u{e68b}'),
            ("toml", '\u{e6b2}'),
            ("markdown", '\u{f48a}'),
            ("json", '\u{e60b}'),
            ("javascript", '\u{e60c}'),
            ("typescript", '\u{e628}'),
            ("html", '\u{e736}'),
            ("css", '\u{e749}'),
        ] {
            assert!(
                face.glyph_index(ch).is_some(),
                "missing {label} glyph U+{:04X}",
                ch as u32
            );
        }

        for glyph in SUPPORTED_GLYPHS {
            assert!(
                face.glyph_index(*glyph).is_some(),
                "missing audited glyph U+{:04X}",
                *glyph as u32
            );
        }

        let expected = SUPPORTED_GLYPHS
            .iter()
            .map(|glyph| *glyph as u32)
            .collect::<std::collections::BTreeSet<_>>();
        let mut actual = std::collections::BTreeSet::new();
        let cmap = face.tables().cmap.expect("icon font has a cmap table");
        for subtable in cmap
            .subtables
            .into_iter()
            .filter(|table| table.is_unicode())
        {
            subtable.codepoints(|codepoint| {
                if subtable.glyph_index(codepoint).is_some() {
                    actual.insert(codepoint);
                }
            });
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn unlicensed_and_source_missing_glyphs_use_the_generic_fallback() {
        assert!(supports_glyph(FOLDER_GLYPH));
        assert!(supports_glyph(GENERIC_FILE_GLYPH));
        assert_eq!(glyph_or_fallback('\u{f303}'), GENERIC_FILE_GLYPH);
        assert_eq!(glyph_or_fallback('\u{f381}'), GENERIC_FILE_GLYPH);
        assert_eq!(glyph_or_fallback('\u{25b2}'), GENERIC_FILE_GLYPH);

        let face = ttf_parser::Face::parse(DEVICON_FONT_BYTES, 0).expect("parse icon font");
        for codepoint in 0xf300..=0xf381 {
            let glyph = char::from_u32(codepoint).expect("valid private-use codepoint");
            assert!(
                face.glyph_index(glyph).is_none(),
                "unlicensed Font Logos glyph U+{codepoint:04X} is bundled"
            );
        }
    }
}
