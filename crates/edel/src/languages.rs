//! The languages Edel OS offers when it asks for one (the installer page,
//! M6.6a polish; Settings' Region page later): each locale as
//! `region.language` takes it, its name in its own language where the
//! image has the script's font, and its English name. The one list; every
//! part reads it here (ADR-010).

use crate::i18n::{n_, tr};

/// A language offered: the locale `region.language` takes, its own name,
/// and its English name, which `tr` translates where it is shown.
pub struct Language {
    /// The locale, such as `de_DE.UTF-8`.
    pub code: &'static str,
    /// The language's name in its own language, or its English name when
    /// the image has no font for its script.
    pub own: &'static str,
    /// The language's English name, marked for translation.
    pub english: &'static str,
    /// The time zone the installer suggests for it, the country's most populous zone.
    pub zone: &'static str,
}

/// Every language offered, in the order people see them: the two English
/// locales first, then the rest by English name.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en_US.UTF-8",
        own: "English (United States)",
        english: n_("English (United States)"),
        zone: "America/New_York",
    },
    Language {
        code: "en_GB.UTF-8",
        own: "English (United Kingdom)",
        english: n_("English (United Kingdom)"),
        zone: "Europe/London",
    },
    Language {
        code: "ar_SA.UTF-8",
        own: "العربية",
        english: n_("Arabic"),
        zone: "Asia/Riyadh",
    },
    Language {
        code: "az_AZ.UTF-8",
        own: "Azərbaycan dili",
        english: n_("Azerbaijani"),
        zone: "Asia/Baku",
    },
    Language {
        code: "bg_BG.UTF-8",
        own: "Български",
        english: n_("Bulgarian"),
        zone: "Europe/Sofia",
    },
    Language {
        code: "cs_CZ.UTF-8",
        own: "Čeština",
        english: n_("Czech"),
        zone: "Europe/Prague",
    },
    Language {
        code: "da_DK.UTF-8",
        own: "Dansk",
        english: n_("Danish"),
        zone: "Europe/Copenhagen",
    },
    Language {
        code: "de_DE.UTF-8",
        own: "Deutsch",
        english: n_("German"),
        zone: "Europe/Berlin",
    },
    Language {
        code: "el_GR.UTF-8",
        own: "Ελληνικά",
        english: n_("Greek"),
        zone: "Europe/Athens",
    },
    Language {
        code: "es_ES.UTF-8",
        own: "Español (España)",
        english: n_("Spanish (Spain)"),
        zone: "Europe/Madrid",
    },
    Language {
        code: "es_MX.UTF-8",
        own: "Español (México)",
        english: n_("Spanish (Mexico)"),
        zone: "America/Mexico_City",
    },
    Language {
        code: "fa_IR.UTF-8",
        own: "فارسی",
        english: n_("Persian"),
        zone: "Asia/Tehran",
    },
    Language {
        code: "fi_FI.UTF-8",
        own: "Suomi",
        english: n_("Finnish"),
        zone: "Europe/Helsinki",
    },
    Language {
        code: "fr_FR.UTF-8",
        own: "Français",
        english: n_("French"),
        zone: "Europe/Paris",
    },
    Language {
        code: "he_IL.UTF-8",
        own: "Hebrew",
        english: n_("Hebrew"),
        zone: "Asia/Jerusalem",
    },
    Language {
        code: "hi_IN.UTF-8",
        own: "हिन्दी",
        english: n_("Hindi"),
        zone: "Asia/Kolkata",
    },
    Language {
        code: "hr_HR.UTF-8",
        own: "Hrvatski",
        english: n_("Croatian"),
        zone: "Europe/Zagreb",
    },
    Language {
        code: "hu_HU.UTF-8",
        own: "Magyar",
        english: n_("Hungarian"),
        zone: "Europe/Budapest",
    },
    Language {
        code: "id_ID.UTF-8",
        own: "Bahasa Indonesia",
        english: n_("Indonesian"),
        zone: "Asia/Jakarta",
    },
    Language {
        code: "it_IT.UTF-8",
        own: "Italiano",
        english: n_("Italian"),
        zone: "Europe/Rome",
    },
    Language {
        code: "ja_JP.UTF-8",
        own: "Japanese",
        english: n_("Japanese"),
        zone: "Asia/Tokyo",
    },
    Language {
        code: "kk_KZ.UTF-8",
        own: "Қазақ тілі",
        english: n_("Kazakh"),
        zone: "Asia/Almaty",
    },
    Language {
        code: "ko_KR.UTF-8",
        own: "Korean",
        english: n_("Korean"),
        zone: "Asia/Seoul",
    },
    Language {
        code: "ms_MY.UTF-8",
        own: "Bahasa Melayu",
        english: n_("Malay"),
        zone: "Asia/Kuala_Lumpur",
    },
    Language {
        code: "nb_NO.UTF-8",
        own: "Norsk bokmål",
        english: n_("Norwegian Bokmål"),
        zone: "Europe/Oslo",
    },
    Language {
        code: "nl_NL.UTF-8",
        own: "Nederlands",
        english: n_("Dutch"),
        zone: "Europe/Amsterdam",
    },
    Language {
        code: "pl_PL.UTF-8",
        own: "Polski",
        english: n_("Polish"),
        zone: "Europe/Warsaw",
    },
    Language {
        code: "pt_BR.UTF-8",
        own: "Português (Brasil)",
        english: n_("Portuguese (Brazil)"),
        zone: "America/Sao_Paulo",
    },
    Language {
        code: "pt_PT.UTF-8",
        own: "Português (Portugal)",
        english: n_("Portuguese (Portugal)"),
        zone: "Europe/Lisbon",
    },
    Language {
        code: "ro_RO.UTF-8",
        own: "Română",
        english: n_("Romanian"),
        zone: "Europe/Bucharest",
    },
    Language {
        code: "ru_RU.UTF-8",
        own: "Русский",
        english: n_("Russian"),
        zone: "Europe/Moscow",
    },
    Language {
        code: "sk_SK.UTF-8",
        own: "Slovenčina",
        english: n_("Slovak"),
        zone: "Europe/Bratislava",
    },
    Language {
        code: "sl_SI.UTF-8",
        own: "Slovenščina",
        english: n_("Slovenian"),
        zone: "Europe/Ljubljana",
    },
    Language {
        code: "sr_RS.UTF-8",
        own: "Српски",
        english: n_("Serbian"),
        zone: "Europe/Belgrade",
    },
    Language {
        code: "sv_SE.UTF-8",
        own: "Svenska",
        english: n_("Swedish"),
        zone: "Europe/Stockholm",
    },
    Language {
        code: "th_TH.UTF-8",
        own: "Thai",
        english: n_("Thai"),
        zone: "Asia/Bangkok",
    },
    Language {
        code: "tr_TR.UTF-8",
        own: "Türkçe",
        english: n_("Turkish"),
        zone: "Europe/Istanbul",
    },
    Language {
        code: "uk_UA.UTF-8",
        own: "Українська",
        english: n_("Ukrainian"),
        zone: "Europe/Kyiv",
    },
    Language {
        code: "uz_UZ.UTF-8",
        own: "O‘zbekcha",
        english: n_("Uzbek"),
        zone: "Asia/Tashkent",
    },
    Language {
        code: "vi_VN.UTF-8",
        own: "Tiếng Việt",
        english: n_("Vietnamese"),
        zone: "Asia/Ho_Chi_Minh",
    },
    Language {
        code: "zh_CN.UTF-8",
        own: "Chinese (Simplified)",
        english: n_("Chinese (Simplified)"),
        zone: "Asia/Shanghai",
    },
    Language {
        code: "zh_TW.UTF-8",
        own: "Chinese (Traditional)",
        english: n_("Chinese (Traditional)"),
        zone: "Asia/Taipei",
    },
];

/// The add-on with the fonts a language's script needs where the image has
/// none (M7.2a): Japanese, Chinese and Korean take `fonts-cjk`, which the
/// installer adds with the language.
pub fn font_addon(code: &str) -> Option<&'static str> {
    ["ja_", "ko_", "zh_"]
        .iter()
        .any(|start| code.starts_with(start))
        .then_some("fonts-cjk")
}

/// The language whose code is `code`.
pub fn find(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|language| language.code == code)
}

/// How a language reads in a list: its own name, then its English name in
/// the person's language after a middle dot when the two differ, such as
/// "Deutsch · German".
pub fn label(language: &Language) -> String {
    if language.own == language.english {
        language.own.to_string()
    } else {
        format!("{} · {}", language.own, tr(language.english))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_languages_bring_their_fonts() {
        assert_eq!(font_addon("ja_JP.UTF-8"), Some("fonts-cjk"));
        assert_eq!(font_addon("zh_TW.UTF-8"), Some("fonts-cjk"));
        assert_eq!(font_addon("de_DE.UTF-8"), None);
    }

    // The installer's answers are the system's own work (the os feature).
    #[cfg(feature = "os")]
    #[test]
    fn every_code_is_a_language_answer_and_none_is_twice() {
        use crate::install::language_answer;
        let mut seen = std::collections::BTreeSet::new();
        for language in LANGUAGES {
            assert!(
                language_answer(language.code).is_ok(),
                "{} is not a language answer",
                language.code
            );
            assert!(
                seen.insert(language.code),
                "{} is listed twice",
                language.code
            );
        }
    }

    #[test]
    fn finds_a_language_by_its_code() {
        assert_eq!(find("de_DE.UTF-8").map(|l| l.english), Some("German"));
        assert!(find("xx_XX.UTF-8").is_none());
    }

    #[test]
    fn labels_show_the_own_name_and_the_english_one() {
        assert_eq!(label(find("de_DE.UTF-8").unwrap()), "Deutsch · German");
        assert_eq!(
            label(find("en_US.UTF-8").unwrap()),
            "English (United States)"
        );
    }
}
