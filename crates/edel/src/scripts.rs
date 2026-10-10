//! The scripts a language needs beyond Latin, Greek and Cyrillic (M5.30c).
//! The lighter stick keeps the fonts of the interface and those three
//! scripts only (M5.30b), so a language whose script has no font says so,
//! with what brings it: `edel report` and `edel status` read it from here.
//! Noto names its fonts by script, which [`has_script`] reads from the file
//! names, so no font is opened.

/// Each language's script beyond the three the image keeps, by the language
/// part of its locale (`ja_JP.UTF-8` is `ja`).
const SCRIPTS: &[(&str, &str)] = &[
    ("ja", "CJK"),
    ("zh", "CJK"),
    ("ko", "CJK"),
    ("ar", "Arabic"),
    ("fa", "Arabic"),
    ("ur", "Arabic"),
    ("he", "Hebrew"),
    ("yi", "Hebrew"),
    ("hi", "Devanagari"),
    ("mr", "Devanagari"),
    ("ne", "Devanagari"),
    ("bn", "Bengali"),
    ("ta", "Tamil"),
    ("th", "Thai"),
    ("ka", "Georgian"),
    ("hy", "Armenian"),
    ("am", "Ethiopic"),
];

/// The script `language` needs beyond Latin, Greek and Cyrillic, such as
/// `CJK` for `ja_JP.UTF-8`; none when it needs none, or is not known.
pub fn script_of(language: &str) -> Option<&'static str> {
    let base = language.split(['_', '.', '@']).next().unwrap_or(language);
    SCRIPTS
        .iter()
        .find(|(name, _)| *name == base)
        .map(|&(_, script)| script)
}

/// Whether one of `font_files` (bare file names) is a Noto font of `script`,
/// the sans or the serif family: `NotoSansCJK-Regular.ttc` and
/// `NotoSerifArabic-Regular.ttf` are, `DejaVuSans.ttf` is not.
pub fn has_script(script: &str, font_files: &[String]) -> bool {
    let prefixes = [format!("NotoSans{script}"), format!("NotoSerif{script}")];
    font_files.iter().any(|file| {
        prefixes
            .iter()
            .any(|prefix| file.starts_with(prefix.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_names_the_script_it_needs_beyond_the_three() {
        assert_eq!(script_of("ja_JP.UTF-8"), Some("CJK"));
        assert_eq!(script_of("ko"), Some("CJK"));
        assert_eq!(script_of("ar"), Some("Arabic"));
        assert_eq!(script_of("hi_IN"), Some("Devanagari"));
        assert_eq!(script_of("am@abegede"), Some("Ethiopic"));
        assert_eq!(script_of("de_DE.UTF-8"), None);
        assert_eq!(script_of("ru"), None);
        assert_eq!(script_of(""), None);
    }

    #[test]
    fn a_noto_font_file_names_its_script_sans_or_serif() {
        let fonts: Vec<String> = [
            "NotoSansCJK-Regular.ttc",
            "NotoSerifArabic-Regular.ttf",
            "NotoSans-Regular.ttf",
            "DejaVuSans.ttf",
        ]
        .map(String::from)
        .to_vec();
        assert!(has_script("CJK", &fonts));
        assert!(has_script("Arabic", &fonts));
        assert!(!has_script("Hebrew", &fonts));
        assert!(!has_script("Thai", &fonts));
        assert!(has_script("CJK", &["NotoSerifCJK-Bold.ttc".to_string()]));
        assert!(!has_script("CJK", &[]));
    }
}
