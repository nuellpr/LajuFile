//! Multi-language catalog, runtime TSV loader, Windows locale detection,
//! RTL layout helpers, and locale-aware number formatting.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use super::ui::format::{eta_seconds, format_eta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberStyle {
    /// English / IDM style: `10200`, `1.50 MB`
    Plain,
    /// European / Indonesian dot-thousands, comma-decimal: `10.200`, `1,50 MB`
    DotComma,
    /// French / Slavic space-thousands, comma-decimal: `10 200`, `1,50 MB`
    SpaceComma,
    /// Comma-thousands, dot-decimal: `10,200`, `1.50 MB`
    CommaDot,
    /// Indian grouping: `1,00,000`, `1.50 MB`
    IndianComma,
}

#[derive(Debug, Clone, Copy)]
pub struct LanguageInfo {
    pub code: &'static str,
    pub badge: &'static str,
    pub native_name: &'static str,
    pub english_name: &'static str,
    pub rtl: bool,
    pub number_style: NumberStyle,
}

/// All 86 Windows 11 display languages, plus built-in catalogs and runtime `.tsv` override support.
pub const CATALOG: &[LanguageInfo] = &[
    LanguageInfo {
        code: "af",
        badge: "ZA",
        native_name: "Afrikaans",
        english_name: "Afrikaans",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "sq",
        badge: "AL",
        native_name: "Shqip",
        english_name: "Albanian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "am",
        badge: "ET",
        native_name: "አማርኛ",
        english_name: "Amharic",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "ar",
        badge: "SA",
        native_name: "العربية",
        english_name: "Arabic",
        rtl: true,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "hy",
        badge: "AM",
        native_name: "Հայերեն",
        english_name: "Armenian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "as",
        badge: "IN",
        native_name: "অসমীয়া",
        english_name: "Assamese",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "az",
        badge: "AZ",
        native_name: "Azərbaycan",
        english_name: "Azerbaijani",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "bn",
        badge: "BD",
        native_name: "বাংলা",
        english_name: "Bangla",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "eu",
        badge: "ES",
        native_name: "Euskara",
        english_name: "Basque",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "be",
        badge: "BY",
        native_name: "Беларуская",
        english_name: "Belarusian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "bs",
        badge: "BA",
        native_name: "Bosanski",
        english_name: "Bosnian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "bg",
        badge: "BG",
        native_name: "Български",
        english_name: "Bulgarian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "ca",
        badge: "ES",
        native_name: "Català",
        english_name: "Catalan",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "zh-cn",
        badge: "CN",
        native_name: "简体中文",
        english_name: "Chinese (Simplified)",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "zh-tw",
        badge: "TW",
        native_name: "繁體中文",
        english_name: "Chinese (Traditional)",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "hr",
        badge: "HR",
        native_name: "Hrvatski",
        english_name: "Croatian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "cs",
        badge: "CZ",
        native_name: "Čeština",
        english_name: "Czech",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "da",
        badge: "DK",
        native_name: "Dansk",
        english_name: "Danish",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "nl",
        badge: "NL",
        native_name: "Nederlands",
        english_name: "Dutch",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "en",
        badge: "US",
        native_name: "English",
        english_name: "English (United States)",
        rtl: false,
        number_style: NumberStyle::Plain,
    },
    LanguageInfo {
        code: "en-gb",
        badge: "GB",
        native_name: "English (UK)",
        english_name: "English (United Kingdom)",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "et",
        badge: "EE",
        native_name: "Eesti",
        english_name: "Estonian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "fil",
        badge: "PH",
        native_name: "Filipino",
        english_name: "Filipino",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "fi",
        badge: "FI",
        native_name: "Suomi",
        english_name: "Finnish",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "fr",
        badge: "FR",
        native_name: "Français",
        english_name: "French (France)",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "fr-ca",
        badge: "CA",
        native_name: "Français (Canada)",
        english_name: "French (Canada)",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "gl",
        badge: "ES",
        native_name: "Galego",
        english_name: "Galician",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "ka",
        badge: "GE",
        native_name: "ქართული",
        english_name: "Georgian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "de",
        badge: "DE",
        native_name: "Deutsch",
        english_name: "German",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "el",
        badge: "GR",
        native_name: "Ελληνικά",
        english_name: "Greek",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "gu",
        badge: "IN",
        native_name: "ગુજરાતી",
        english_name: "Gujarati",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "he",
        badge: "IL",
        native_name: "עברית",
        english_name: "Hebrew",
        rtl: true,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "hi",
        badge: "IN",
        native_name: "हिन्दी",
        english_name: "Hindi",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "hu",
        badge: "HU",
        native_name: "Magyar",
        english_name: "Hungarian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "is",
        badge: "IS",
        native_name: "Íslenska",
        english_name: "Icelandic",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "id",
        badge: "ID",
        native_name: "Bahasa Indonesia",
        english_name: "Indonesian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "ga",
        badge: "IE",
        native_name: "Gaeilge",
        english_name: "Irish",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "it",
        badge: "IT",
        native_name: "Italiano",
        english_name: "Italian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "ja",
        badge: "JP",
        native_name: "日本語",
        english_name: "Japanese",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "kn",
        badge: "IN",
        native_name: "ಕನ್ನಡ",
        english_name: "Kannada",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "kk",
        badge: "KZ",
        native_name: "Қазақ тілі",
        english_name: "Kazakh",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "km",
        badge: "KH",
        native_name: "ខ្មែរ",
        english_name: "Khmer",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "kok",
        badge: "IN",
        native_name: "कोंकणी",
        english_name: "Konkani",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "ko",
        badge: "KR",
        native_name: "한국어",
        english_name: "Korean",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "ky",
        badge: "KG",
        native_name: "Кыргызча",
        english_name: "Kyrgyz",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "lo",
        badge: "LA",
        native_name: "ລາວ",
        english_name: "Lao",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "lv",
        badge: "LV",
        native_name: "Latviešu",
        english_name: "Latvian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "lt",
        badge: "LT",
        native_name: "Lietuvių",
        english_name: "Lithuanian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "lb",
        badge: "LU",
        native_name: "Lëtzebuergesch",
        english_name: "Luxembourgish",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "mk",
        badge: "MK",
        native_name: "Македонски",
        english_name: "Macedonian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "ms",
        badge: "MY",
        native_name: "Bahasa Melayu",
        english_name: "Malay",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "ml",
        badge: "IN",
        native_name: "മലയാളം",
        english_name: "Malayalam",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "mt",
        badge: "MT",
        native_name: "Malti",
        english_name: "Maltese",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "mi",
        badge: "NZ",
        native_name: "Te Reo Māori",
        english_name: "Maori",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "mr",
        badge: "IN",
        native_name: "मराठी",
        english_name: "Marathi",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "mn",
        badge: "MN",
        native_name: "Монгол",
        english_name: "Mongolian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "ne",
        badge: "NP",
        native_name: "नेपाली",
        english_name: "Nepali",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "nb",
        badge: "NO",
        native_name: "Norsk Bokmål",
        english_name: "Norwegian Bokmål",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "nn",
        badge: "NO",
        native_name: "Norsk Nynorsk",
        english_name: "Norwegian Nynorsk",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "or",
        badge: "IN",
        native_name: "ଓଡ଼ିଆ",
        english_name: "Odia",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "fa",
        badge: "IR",
        native_name: "فارسی",
        english_name: "Persian",
        rtl: true,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "pl",
        badge: "PL",
        native_name: "Polski",
        english_name: "Polish",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "pt-br",
        badge: "BR",
        native_name: "Português (Brasil)",
        english_name: "Portuguese (Brazil)",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "pt-pt",
        badge: "PT",
        native_name: "Português (Portugal)",
        english_name: "Portuguese (Portugal)",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "pa",
        badge: "IN",
        native_name: "ਪੰਜਾਬੀ",
        english_name: "Punjabi",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "ro",
        badge: "RO",
        native_name: "Română",
        english_name: "Romanian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "ru",
        badge: "RU",
        native_name: "Русский",
        english_name: "Russian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "gd",
        badge: "GB",
        native_name: "Gàidhlig",
        english_name: "Scottish Gaelic",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "sr-cyrl",
        badge: "RS",
        native_name: "Српски",
        english_name: "Serbian (Cyrillic)",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "sr-latn",
        badge: "RS",
        native_name: "Srpski",
        english_name: "Serbian (Latin)",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "si",
        badge: "LK",
        native_name: "සිංහල",
        english_name: "Sinhala",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "sk",
        badge: "SK",
        native_name: "Slovenčina",
        english_name: "Slovak",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "sl",
        badge: "SI",
        native_name: "Slovenščina",
        english_name: "Slovenian",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "es",
        badge: "ES",
        native_name: "Español (España)",
        english_name: "Spanish (Spain)",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "es-mx",
        badge: "MX",
        native_name: "Español (México)",
        english_name: "Spanish (Mexico)",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "sw",
        badge: "KE",
        native_name: "Kiswahili",
        english_name: "Swahili",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "sv",
        badge: "SE",
        native_name: "Svenska",
        english_name: "Swedish",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "ta",
        badge: "IN",
        native_name: "தமிழ்",
        english_name: "Tamil",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "tt",
        badge: "RU",
        native_name: "Татар",
        english_name: "Tatar",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "te",
        badge: "IN",
        native_name: "తెలుగు",
        english_name: "Telugu",
        rtl: false,
        number_style: NumberStyle::IndianComma,
    },
    LanguageInfo {
        code: "th",
        badge: "TH",
        native_name: "ไทย",
        english_name: "Thai",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "tr",
        badge: "TR",
        native_name: "Türkçe",
        english_name: "Turkish",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "uk",
        badge: "UA",
        native_name: "Українська",
        english_name: "Ukrainian",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "ur",
        badge: "PK",
        native_name: "اردو",
        english_name: "Urdu",
        rtl: true,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "ug",
        badge: "CN",
        native_name: "ئۇيغۇرچە",
        english_name: "Uyghur",
        rtl: true,
        number_style: NumberStyle::CommaDot,
    },
    LanguageInfo {
        code: "uz",
        badge: "UZ",
        native_name: "O‘zbek",
        english_name: "Uzbek",
        rtl: false,
        number_style: NumberStyle::SpaceComma,
    },
    LanguageInfo {
        code: "vi",
        badge: "VN",
        native_name: "Tiếng Việt",
        english_name: "Vietnamese",
        rtl: false,
        number_style: NumberStyle::DotComma,
    },
    LanguageInfo {
        code: "cy",
        badge: "GB",
        native_name: "Cymraeg",
        english_name: "Welsh",
        rtl: false,
        number_style: NumberStyle::CommaDot,
    },
];

pub fn find_language(code: &str) -> Option<&'static LanguageInfo> {
    let lower = code.trim().to_ascii_lowercase();
    CATALOG
        .iter()
        .find(|entry| entry.code == lower)
        .or_else(|| {
            let base = lower.split('-').next()?;
            CATALOG.iter().find(|entry| entry.code == base)
        })
}

pub fn detect_windows_language() -> &'static str {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        let lcid = unsafe { GetUserDefaultUILanguage() };
        match lcid {
            0x0421 => "id",
            0x0804 | 0x1004 => "zh-cn",
            0x0404 | 0x0c04 | 0x1404 => "zh-tw",
            0x0416 => "pt-br",
            0x0816 => "pt-pt",
            0x080a => "es-mx",
            0x0c0c => "fr-ca",
            0x0809 => "en-gb",
            other => match other & 0x03ff {
                0x01 => "ar",
                0x02 => "bg",
                0x03 => "ca",
                0x05 => "cs",
                0x06 => "da",
                0x07 => "de",
                0x08 => "el",
                0x09 => "en",
                0x0a => "es",
                0x0b => "fi",
                0x0c => "fr",
                0x0d => "he",
                0x0e => "hu",
                0x10 => "it",
                0x11 => "ja",
                0x12 => "ko",
                0x13 => "nl",
                0x14 => "nb",
                0x15 => "pl",
                0x16 => "pt-br",
                0x18 => "ro",
                0x19 => "ru",
                0x1a => "hr",
                0x1b => "sk",
                0x1d => "sv",
                0x1e => "th",
                0x1f => "tr",
                0x20 => "ur",
                0x21 => "id",
                0x22 => "uk",
                0x24 => "sl",
                0x25 => "et",
                0x26 => "lv",
                0x27 => "lt",
                0x29 => "fa",
                0x2a => "vi",
                0x39 => "hi",
                0x3e => "ms",
                _ => "en",
            },
        }
    }
    #[cfg(not(windows))]
    {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(raw) = std::env::var(var) {
                let norm = raw
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .replace('_', "-")
                    .to_ascii_lowercase();
                if norm.is_empty() || norm == "c" || norm == "posix" {
                    continue;
                }
                if find_language(&norm).is_some() {
                    if let Some(info) = find_language(&norm) {
                        return info.code;
                    }
                }
                if let Some((base, _)) = norm.split_once('-')
                    && let Some(info) = find_language(base)
                {
                    return info.code;
                }
            }
        }
        "en"
    }
}

pub fn effective_code(setting: &str) -> String {
    let clean = setting.trim().to_ascii_lowercase();
    if clean.is_empty() || clean == "system" {
        detect_windows_language().to_string()
    } else {
        clean
    }
}

pub fn is_rtl(lang_code: &str) -> bool {
    let eff = effective_code(lang_code);
    find_language(&eff).is_some_and(|info| info.rtl)
}

pub fn number_style_for(lang_code: &str) -> NumberStyle {
    let eff = effective_code(lang_code);
    find_language(&eff)
        .map(|info| info.number_style)
        .unwrap_or(NumberStyle::Plain)
}

pub fn set_ctx_lang(ctx: &egui::Context, lang: &str) {
    let id = egui::Id::unique("lajufile_active_lang");
    ctx.data_mut(|data| data.insert_temp(id, effective_code(lang)));
}

pub fn ctx_lang(ctx: &egui::Context) -> String {
    let id = egui::Id::unique("lajufile_active_lang");
    ctx.data(|data| data.get_temp::<String>(id))
        .unwrap_or_else(|| "en".to_string())
}

pub fn t_ctx<'a>(ctx: &egui::Context, en: &'a str) -> Cow<'a, str> {
    let lang = ctx_lang(ctx);
    translate(&lang, en)
}

pub fn has_builtin_catalog(code: &str) -> bool {
    matches!(
        code.split('-').next().unwrap_or(code),
        "id" | "ms"
            | "de"
            | "fr"
            | "es"
            | "pt"
            | "it"
            | "nl"
            | "pl"
            | "tr"
            | "ru"
            | "uk"
            | "ar"
            | "hi"
            | "th"
            | "vi"
            | "ja"
            | "ko"
            | "en"
    ) || matches!(code, "zh-cn" | "zh-tw" | "pt-br")
}

/// Registers system fallback fonts (Segoe UI, Tahoma, CJK, Devanagari) so non-Latin scripts
/// render crisply across all languages without missing-glyph boxes.
pub fn install_system_fonts(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        const FONT_PATHS: &[(&str, &str)] = &[
            ("win_segoeui", r"C:\Windows\Fonts\segoeui.ttf"),
            ("win_tahoma", r"C:\Windows\Fonts\tahoma.ttf"),
            ("win_malgun", r"C:\Windows\Fonts\malgun.ttf"),
            ("win_nirmala", r"C:\Windows\Fonts\Nirmala.ttf"),
            ("win_msyh", r"C:\Windows\Fonts\msyh.ttc"),
            ("win_meiryo", r"C:\Windows\Fonts\meiryo.ttc"),
        ];
        let mut defs = egui::FontDefinitions::default();
        let mut added = false;
        for (name, path) in FONT_PATHS {
            if let Ok(bytes) = std::fs::read(path) {
                defs.font_data.insert(
                    (*name).to_string(),
                    egui::FontData::from_owned(bytes).into(),
                );
                if let Some(list) = defs.families.get_mut(&egui::FontFamily::Proportional) {
                    list.push((*name).to_string());
                }
                if let Some(list) = defs.families.get_mut(&egui::FontFamily::Monospace) {
                    list.push((*name).to_string());
                }
                added = true;
            }
        }
        if added {
            ctx.set_fonts(defs);
        }
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "macos")]
        const FONT_PATHS: &[(&str, &str)] = &[
            (
                "sys_arial_unicode",
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            ),
            ("sys_hiragino", "/System/Library/Fonts/Hiragino Sans GB.ttc"),
            ("sys_apple_sd", "/System/Library/Fonts/AppleSDGothicNeo.ttc"),
        ];
        #[cfg(not(target_os = "macos"))]
        const FONT_PATHS: &[(&str, &str)] = &[
            (
                "sys_noto_sans",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            ),
            (
                "sys_dejavu",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ),
        ];
        let mut defs = egui::FontDefinitions::default();
        let mut added = false;
        for (name, path) in FONT_PATHS {
            if let Ok(bytes) = std::fs::read(path) {
                defs.font_data.insert(
                    (*name).to_string(),
                    egui::FontData::from_owned(bytes).into(),
                );
                if let Some(list) = defs.families.get_mut(&egui::FontFamily::Proportional) {
                    list.push((*name).to_string());
                }
                if let Some(list) = defs.families.get_mut(&egui::FontFamily::Monospace) {
                    list.push((*name).to_string());
                }
                added = true;
            }
        }
        if added {
            ctx.set_fonts(defs);
        }
    }
}

pub fn format_grouped_int(value: u64, style: NumberStyle) -> String {
    let digits = value.to_string();
    if style == NumberStyle::Plain || digits.len() <= 3 {
        return digits;
    }
    let sep = match style {
        NumberStyle::Plain => return digits,
        NumberStyle::DotComma => '.',
        NumberStyle::SpaceComma => ' ',
        NumberStyle::CommaDot | NumberStyle::IndianComma => ',',
    };
    if style == NumberStyle::IndianComma {
        let rev: Vec<char> = digits.chars().rev().collect();
        let mut out = String::with_capacity(digits.len() + 4);
        for (idx, ch) in rev.iter().enumerate() {
            if idx == 3 || (idx > 3 && (idx - 3) % 2 == 0) {
                out.push(sep);
            }
            out.push(*ch);
        }
        return out.chars().rev().collect();
    }
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(sep);
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

pub fn format_decimal_locale(formatted_en: String, lang: &str) -> String {
    match number_style_for(lang) {
        NumberStyle::DotComma | NumberStyle::SpaceComma => formatted_en.replace('.', ","),
        NumberStyle::Plain | NumberStyle::CommaDot | NumberStyle::IndianComma => formatted_en,
    }
}

pub fn format_size_locale(bytes: u64, lang: &str) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!(
            "{} {}",
            format_grouped_int(bytes, number_style_for(lang)),
            UNITS[0]
        );
    }
    let raw = format!("{value:.1}");
    format!("{} {}", format_decimal_locale(raw, lang), UNITS[unit])
}

pub fn format_speed_locale(bytes_per_sec: Option<u64>, lang: &str) -> String {
    match bytes_per_sec {
        Some(speed) if speed > 0 => format!("{}/s", format_size_locale(speed, lang)),
        _ => "—".to_string(),
    }
}

pub fn format_percent_locale(fraction: f32, lang: &str) -> String {
    let pct = format!("{:.0}%", (fraction * 100.0).clamp(0.0, 100.0));
    format_decimal_locale(pct, lang)
}

pub fn format_eta_locale(size: Option<u64>, downloaded: u64, speed: Option<u64>) -> String {
    format_eta(eta_seconds(size, downloaded, speed.unwrap_or(0)))
}

static EXTERNAL_TSV: OnceLock<RwLock<HashMap<String, HashMap<String, String>>>> = OnceLock::new();

pub fn parse_tsv_catalog(raw: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut cols = line.splitn(3, '\t');
        let _context = cols.next();
        let Some(source) = cols.next() else {
            continue;
        };
        let Some(translation) = cols.next() else {
            continue;
        };
        let src = source.replace('&', "");
        let trn = translation.trim().replace('&', "");
        if !src.is_empty() && !trn.is_empty() {
            map.insert(src, trn);
        }
    }
    map
}

fn find_tsv_path(code: &str) -> Option<PathBuf> {
    let rel = PathBuf::from("i18n").join(format!("{code}.tsv"));
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join(&rel));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(&rel));
    }
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
    {
        candidates.push(root.join(&rel));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn lookup_external_tsv(code: &str, en: &str) -> Option<String> {
    let cache = EXTERNAL_TSV.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(guard) = cache.read()
        && let Some(map) = guard.get(code)
    {
        return map.get(en).cloned();
    }
    let map = find_tsv_path(code)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|raw| parse_tsv_catalog(&raw))
        .unwrap_or_default();
    let result = map.get(en).cloned();
    if let Ok(mut guard) = cache.write() {
        guard.insert(code.to_string(), map);
    }
    result
}

pub fn translate<'a>(lang_code: &str, en: &'a str) -> Cow<'a, str> {
    let code = effective_code(lang_code);
    if code == "en" || code.starts_with("en-") {
        return Cow::Borrowed(en);
    }
    if let Some(ext) = lookup_external_tsv(&code, en) {
        return Cow::Owned(ext);
    }
    if let Some(hit) = builtin_lookup(&code, en) {
        return Cow::Borrowed(hit);
    }
    if let Some((base, _)) = code.split_once('-')
        && let Some(hit) = builtin_lookup(base, en)
    {
        return Cow::Borrowed(hit);
    }
    Cow::Borrowed(en)
}

fn builtin_lookup(code: &str, en: &str) -> Option<&'static str> {
    match code {
        "id" => match en {
            "File" => Some("Berkas"),
            "Tasks" => Some("Tugas"),
            "View" => Some("Tampilan"),
            "Help" => Some("Bantuan"),
            "Add URL" => Some("Tambah URL"),
            "Add URL..." => Some("Tambah URL..."),
            "Add new download..." => Some("Tambah unduhan baru..."),
            "Add batch download..." => Some("Tambah unduhan batch..."),
            "Find download..." => Some("Cari unduhan..."),
            "Select all" => Some("Pilih semua"),
            "Export links (.txt)" => Some("Ekspor tautan (.txt)"),
            "Batch" => Some("Batch"),
            "Resume" => Some("Lanjutkan"),
            "Stop" => Some("Hentikan"),
            "Stop All" | "Stop all" => Some("Hentikan Semua"),
            "Resume all" => Some("Lanjutkan semua"),
            "Delete" => Some("Hapus"),
            "Delete Completed" | "Delete completed" => Some("Hapus Selesai"),
            "Options" => Some("Opsi"),
            "Settings..." => Some("Pengaturan..."),
            "Exit" => Some("Keluar"),
            "Light" => Some("Terang"),
            "Dark" => Some("Gelap"),
            "Interface size" => Some("Ukuran antarmuka"),
            "Language..." => Some("Bahasa..."),
            "Browser extension wizard..." => Some("Wizard ekstensi browser..."),
            "LIBRARY" => Some("PUSTAKA"),
            "All downloads" => Some("Semua unduhan"),
            "Unfinished" => Some("Belum selesai"),
            "Finished" => Some("Selesai"),
            "CATEGORIES" => Some("KATEGORI"),
            "Video" => Some("Video"),
            "Music" => Some("Musik"),
            "Documents" => Some("Dokumen"),
            "Programs" => Some("Program"),
            "Compressed" => Some("Arsip"),
            "Other" => Some("Lainnya"),
            "NAME" => Some("NAMA"),
            "SIZE" => Some("UKURAN"),
            "STATUS" => Some("STATUS"),
            "PROGRESS" => Some("PROGRES"),
            "SPEED" => Some("KECEPATAN"),
            "ETA" => Some("ESTIMASI"),
            "ADDED" => Some("DITAMBAHKAN"),
            "Queued" => Some("Antrean"),
            "Downloading" => Some("Mengunduh"),
            "Paused" => Some("Dijeda"),
            "Completed" => Some("Selesai"),
            "Error" => Some("Gagal"),
            "Search name, URL, status..." => Some("Cari nama, URL, status..."),
            "No downloads yet" => Some("Belum ada unduhan"),
            "Add a link above or let your browser extension send one over." => {
                Some("Tambahkan tautan di atas atau biarkan ekstensi browser mengirimnya.")
            }
            "Settings" => Some("Pengaturan"),
            "General" => Some("Umum"),
            "Downloads" => Some("Unduhan"),
            "Browser capture" => Some("Tangkap browser"),
            "Appearance" => Some("Tampilan"),
            "About" => Some("Tentang"),
            "Done" => Some("Selesai"),
            "Cancel" => Some("Batal"),
            "Close" => Some("Tutup"),
            "Open" => Some("Buka"),
            "Open with..." => Some("Buka dengan..."),
            "Open folder" => Some("Buka folder"),
            "Download File Info" => Some("Info Berkas Unduhan"),
            "Download complete" => Some("Unduhan selesai"),
            "Start download" => Some("Mulai unduh"),
            "Download later" => Some("Unduh nanti"),
            "Add LajuFile to your browser" => Some("Tambahkan LajuFile ke browser Anda"),
            "Catch downloads from your browser" => Some("Tangkap unduhan dari browser Anda"),
            "Choose language" => Some("Pilih bahasa"),
            "Windows' language" => Some("Bahasa Windows"),
            "Apply" => Some("Terapkan"),
            _ => None,
        },
        "ms" => match en {
            "File" => Some("Fail"),
            "Tasks" => Some("Tugas"),
            "View" => Some("Paparan"),
            "Help" => Some("Bantuan"),
            "Add URL" => Some("Tambah URL"),
            "Batch" => Some("Berkumpulan"),
            "Resume" => Some("Sambung"),
            "Stop" => Some("Henti"),
            "Stop All" | "Stop all" => Some("Henti Semua"),
            "Delete" => Some("Padam"),
            "Delete Completed" | "Delete completed" => Some("Padam Selesai"),
            "Options" => Some("Pilihan"),
            "Language..." => Some("Bahasa..."),
            "All downloads" => Some("Semua muat turun"),
            "Unfinished" => Some("Belum selesai"),
            "Finished" => Some("Selesai"),
            "Completed" => Some("Selesai"),
            "Downloading" => Some("Memuat turun"),
            "Paused" => Some("Dijeda"),
            "Settings" => Some("Tetapan"),
            "Close" => Some("Tutup"),
            "Open" => Some("Buka"),
            "Open with..." => Some("Buka dengan..."),
            "Open folder" => Some("Buka folder"),
            "Choose language" => Some("Pilih bahasa"),
            _ => None,
        },
        "ar" => match en {
            "File" => Some("ملف"),
            "Tasks" => Some("المهام"),
            "View" => Some("عرض"),
            "Help" => Some("مساعدة"),
            "Add URL" => Some("إضافة رابط"),
            "Add URL..." => Some("إضافة رابط..."),
            "Batch" => Some("دفعة"),
            "Resume" => Some("استئناف"),
            "Stop" => Some("إيقاف"),
            "Stop All" | "Stop all" => Some("إيقاف الكل"),
            "Resume all" => Some("استئناف الكل"),
            "Delete" => Some("حذف"),
            "Delete Completed" | "Delete completed" => Some("حذف المكتمل"),
            "Options" => Some("خيارات"),
            "Settings..." => Some("الإعدادات..."),
            "Exit" => Some("خروج"),
            "Light" => Some("فاتح"),
            "Dark" => Some("داكن"),
            "Interface size" => Some("حجم الواجهة"),
            "Language..." => Some("اللغة..."),
            "Browser extension wizard..." => Some("معالج إضافة المتصفح..."),
            "LIBRARY" => Some("المكتبة"),
            "All downloads" => Some("كل التنزيلات"),
            "Unfinished" => Some("غير مكتملة"),
            "Finished" => Some("مكتملة"),
            "CATEGORIES" => Some("التصنيفات"),
            "Video" => Some("فيديو"),
            "Music" => Some("موسيقى"),
            "Documents" => Some("مستندات"),
            "Programs" => Some("برامج"),
            "Compressed" => Some("مضغوطات"),
            "Other" => Some("أخرى"),
            "NAME" => Some("الاسم"),
            "SIZE" => Some("الحجم"),
            "STATUS" => Some("الحالة"),
            "PROGRESS" => Some("التقدم"),
            "SPEED" => Some("السرعة"),
            "ETA" => Some("الوقت المتبقي"),
            "ADDED" => Some("أضيف"),
            "Queued" => Some("في الانتظار"),
            "Downloading" => Some("جارٍ التنزيل"),
            "Paused" => Some("متوقف"),
            "Completed" => Some("مكتمل"),
            "Error" => Some("خطأ"),
            "Search name, URL, status..." => Some("ابحث عن الاسم أو الرابط أو الحالة..."),
            "No downloads yet" => Some("لا توجد تنزيلات بعد"),
            "Settings" => Some("الإعدادات"),
            "General" => Some("عام"),
            "Downloads" => Some("التنزيلات"),
            "Browser capture" => Some("التقاط المتصفح"),
            "Appearance" => Some("المظهر"),
            "About" => Some("حول"),
            "Done" => Some("تم"),
            "Cancel" => Some("إلغاء"),
            "Close" => Some("إغلاق"),
            "Open" => Some("فتح"),
            "Open with..." => Some("فتح باستخدام..."),
            "Open folder" => Some("فتح المجلد"),
            "Download File Info" => Some("معلومات ملف التنزيل"),
            "Download complete" => Some("اكتمل التنزيل"),
            "Start download" => Some("بدء التنزيل"),
            "Download later" => Some("التنزيل لاحقاً"),
            "Add LajuFile to your browser" => Some("إضافة LajuFile إلى متصفحك"),
            "Catch downloads from your browser" => Some("التقاط التنزيلات من المتصفح"),
            "Choose language" => Some("اختر اللغة"),
            "Windows' language" => Some("لغة ويندوز"),
            "Apply" => Some("تطبيق"),
            _ => None,
        },
        "de" => match en {
            "File" => Some("Datei"),
            "Tasks" => Some("Aufgaben"),
            "View" => Some("Ansicht"),
            "Help" => Some("Hilfe"),
            "Add URL" => Some("URL hinzufügen"),
            "Batch" => Some("Stapel"),
            "Resume" => Some("Fortsetzen"),
            "Stop" => Some("Stopp"),
            "Stop All" | "Stop all" => Some("Alle stoppen"),
            "Delete" => Some("Löschen"),
            "Delete Completed" | "Delete completed" => Some("Fertige löschen"),
            "Options" => Some("Optionen"),
            "Language..." => Some("Sprache..."),
            "All downloads" => Some("Alle Downloads"),
            "Unfinished" => Some("Unvollständig"),
            "Finished" => Some("Fertig"),
            "Video" => Some("Video"),
            "Music" => Some("Musik"),
            "Documents" => Some("Dokumente"),
            "Programs" => Some("Programme"),
            "Compressed" => Some("Archive"),
            "Other" => Some("Sonstiges"),
            "Downloading" => Some("Lädt"),
            "Paused" => Some("Pausiert"),
            "Completed" => Some("Abgeschlossen"),
            "Queued" => Some("Warteschlange"),
            "Settings" => Some("Einstellungen"),
            "Close" => Some("Schließen"),
            "Open" => Some("Öffnen"),
            "Open with..." => Some("Öffnen mit..."),
            "Open folder" => Some("Ordner öffnen"),
            "Start download" => Some("Download starten"),
            "Download later" => Some("Später laden"),
            "Choose language" => Some("Sprache wählen"),
            "Windows' language" => Some("Windows-Sprache"),
            "Apply" => Some("Übernehmen"),
            _ => None,
        },
        "fr" => match en {
            "File" => Some("Fichier"),
            "Tasks" => Some("Tâches"),
            "View" => Some("Affichage"),
            "Help" => Some("Aide"),
            "Add URL" => Some("Ajouter URL"),
            "Batch" => Some("Lot"),
            "Resume" => Some("Reprendre"),
            "Stop" => Some("Arrêter"),
            "Stop All" | "Stop all" => Some("Tout arrêter"),
            "Delete" => Some("Supprimer"),
            "Delete Completed" | "Delete completed" => Some("Supprimer terminés"),
            "Options" => Some("Options"),
            "Language..." => Some("Langue..."),
            "All downloads" => Some("Tous les téléchargements"),
            "Unfinished" => Some("En cours"),
            "Finished" => Some("Terminés"),
            "Downloading" => Some("Téléchargement"),
            "Paused" => Some("En pause"),
            "Completed" => Some("Terminé"),
            "Queued" => Some("En file"),
            "Settings" => Some("Paramètres"),
            "Close" => Some("Fermer"),
            "Open" => Some("Ouvrir"),
            "Open with..." => Some("Ouvrir avec..."),
            "Open folder" => Some("Ouvrir le dossier"),
            "Start download" => Some("Démarrer"),
            "Download later" => Some("Plus tard"),
            "Choose language" => Some("Choisir la langue"),
            "Windows' language" => Some("Langue de Windows"),
            "Apply" => Some("Appliquer"),
            _ => None,
        },
        "es" => match en {
            "File" => Some("Archivo"),
            "Tasks" => Some("Tareas"),
            "View" => Some("Ver"),
            "Help" => Some("Ayuda"),
            "Add URL" => Some("Añadir URL"),
            "Batch" => Some("Lote"),
            "Resume" => Some("Reanudar"),
            "Stop" => Some("Detener"),
            "Stop All" | "Stop all" => Some("Detener todo"),
            "Delete" => Some("Eliminar"),
            "Delete Completed" | "Delete completed" => Some("Borrar completados"),
            "Options" => Some("Opciones"),
            "Language..." => Some("Idioma..."),
            "All downloads" => Some("Todas las descargas"),
            "Unfinished" => Some("Incompletas"),
            "Finished" => Some("Completadas"),
            "Downloading" => Some("Descargando"),
            "Paused" => Some("En pausa"),
            "Completed" => Some("Completado"),
            "Settings" => Some("Configuración"),
            "Close" => Some("Cerrar"),
            "Open" => Some("Abrir"),
            "Open with..." => Some("Abrir con..."),
            "Open folder" => Some("Abrir carpeta"),
            "Choose language" => Some("Elegir idioma"),
            "Windows' language" => Some("Idioma de Windows"),
            "Apply" => Some("Aplicar"),
            _ => None,
        },
        "pt-br" | "pt" => match en {
            "File" => Some("Arquivo"),
            "Tasks" => Some("Tarefas"),
            "View" => Some("Exibir"),
            "Help" => Some("Ajuda"),
            "Add URL" => Some("Adicionar URL"),
            "Batch" => Some("Lote"),
            "Resume" => Some("Retomar"),
            "Stop" => Some("Parar"),
            "Stop All" | "Stop all" => Some("Parar tudo"),
            "Delete" => Some("Excluir"),
            "Options" => Some("Opções"),
            "Language..." => Some("Idioma..."),
            "All downloads" => Some("Todos os downloads"),
            "Unfinished" => Some("Inacabados"),
            "Finished" => Some("Concluídos"),
            "Downloading" => Some("Baixando"),
            "Paused" => Some("Pausado"),
            "Completed" => Some("Concluído"),
            "Settings" => Some("Configurações"),
            "Close" => Some("Fechar"),
            "Open" => Some("Abrir"),
            "Open with..." => Some("Abrir com..."),
            "Open folder" => Some("Abrir pasta"),
            "Choose language" => Some("Escolher idioma"),
            "Windows' language" => Some("Idioma do Windows"),
            "Apply" => Some("Aplicar"),
            _ => None,
        },
        "zh-cn" => match en {
            "File" => Some("文件"),
            "Tasks" => Some("任务"),
            "View" => Some("视图"),
            "Help" => Some("帮助"),
            "Add URL" => Some("添加链接"),
            "Batch" => Some("批量"),
            "Resume" => Some("继续"),
            "Stop" => Some("停止"),
            "Stop All" | "Stop all" => Some("全部停止"),
            "Delete" => Some("删除"),
            "Delete Completed" | "Delete completed" => Some("删除已完成"),
            "Options" => Some("选项"),
            "Language..." => Some("语言..."),
            "All downloads" => Some("全部下载"),
            "Unfinished" => Some("未完成"),
            "Finished" => Some("已完成"),
            "Downloading" => Some("下载中"),
            "Paused" => Some("已暂停"),
            "Completed" => Some("已完成"),
            "Queued" => Some("队列中"),
            "Settings" => Some("设置"),
            "Close" => Some("关闭"),
            "Open" => Some("打开"),
            "Open with..." => Some("打开方式..."),
            "Open folder" => Some("打开文件夹"),
            "Start download" => Some("开始下载"),
            "Download later" => Some("稍后下载"),
            "Choose language" => Some("选择语言"),
            "Windows' language" => Some("跟随 Windows 语言"),
            "Apply" => Some("应用"),
            _ => None,
        },
        "zh-tw" => match en {
            "File" => Some("檔案"),
            "Tasks" => Some("工作"),
            "View" => Some("檢視"),
            "Help" => Some("說明"),
            "Add URL" => Some("新增網址"),
            "Batch" => Some("批次"),
            "Resume" => Some("繼續"),
            "Stop" => Some("停止"),
            "Stop All" | "Stop all" => Some("全部停止"),
            "Delete" => Some("刪除"),
            "Options" => Some("選項"),
            "Language..." => Some("語言..."),
            "All downloads" => Some("所有下載"),
            "Unfinished" => Some("未完成"),
            "Finished" => Some("已完成"),
            "Downloading" => Some("下載中"),
            "Paused" => Some("已暫停"),
            "Completed" => Some("已完成"),
            "Settings" => Some("設定"),
            "Close" => Some("關閉"),
            "Open" => Some("開啟"),
            "Open with..." => Some("開啟檔案..."),
            "Open folder" => Some("開啟資料夾"),
            "Choose language" => Some("選擇語言"),
            "Windows' language" => Some("Windows 語言"),
            "Apply" => Some("套用"),
            _ => None,
        },
        "ja" => match en {
            "File" => Some("ファイル"),
            "Tasks" => Some("タスク"),
            "View" => Some("表示"),
            "Help" => Some("ヘルプ"),
            "Add URL" => Some("URLを追加"),
            "Batch" => Some("一括"),
            "Resume" => Some("再開"),
            "Stop" => Some("停止"),
            "Stop All" | "Stop all" => Some("すべて停止"),
            "Delete" => Some("削除"),
            "Options" => Some("オプション"),
            "Language..." => Some("言語..."),
            "All downloads" => Some("すべてのダウンロード"),
            "Unfinished" => Some("未完了"),
            "Finished" => Some("完了"),
            "Downloading" => Some("ダウンロード中"),
            "Paused" => Some("一時停止"),
            "Completed" => Some("完了"),
            "Settings" => Some("設定"),
            "Close" => Some("閉じる"),
            "Open" => Some("開く"),
            "Open with..." => Some("プログラムから開く..."),
            "Open folder" => Some("フォルダーを開く"),
            "Start download" => Some("ダウンロード開始"),
            "Download later" => Some("後でダウンロード"),
            "Choose language" => Some("言語を選択"),
            "Windows' language" => Some("Windows の言語"),
            "Apply" => Some("適用"),
            _ => None,
        },
        "ko" => match en {
            "File" => Some("파일"),
            "Tasks" => Some("작업"),
            "View" => Some("보기"),
            "Help" => Some("도움말"),
            "Add URL" => Some("URL 추가"),
            "Batch" => Some("일괄"),
            "Resume" => Some("재개"),
            "Stop" => Some("정지"),
            "Stop All" | "Stop all" => Some("모두 정지"),
            "Delete" => Some("삭제"),
            "Options" => Some("옵션"),
            "Language..." => Some("언어..."),
            "All downloads" => Some("모든 다운로드"),
            "Unfinished" => Some("미완료"),
            "Finished" => Some("완료"),
            "Downloading" => Some("다운로드 중"),
            "Paused" => Some("일시 정지"),
            "Completed" => Some("완료"),
            "Settings" => Some("설정"),
            "Close" => Some("닫기"),
            "Open" => Some("열기"),
            "Open with..." => Some("연결 프로그램..."),
            "Open folder" => Some("폴더 열기"),
            "Choose language" => Some("언어 선택"),
            "Windows' language" => Some("Windows 언어"),
            "Apply" => Some("적용"),
            _ => None,
        },
        "ru" => match en {
            "File" => Some("Файл"),
            "Tasks" => Some("Задачи"),
            "View" => Some("Вид"),
            "Help" => Some("Справка"),
            "Add URL" => Some("Добавить URL"),
            "Batch" => Some("Пакет"),
            "Resume" => Some("Продолжить"),
            "Stop" => Some("Стоп"),
            "Stop All" | "Stop all" => Some("Остановить все"),
            "Delete" => Some("Удалить"),
            "Options" => Some("Настройки"),
            "Language..." => Some("Язык..."),
            "All downloads" => Some("Все загрузки"),
            "Unfinished" => Some("Незавершённые"),
            "Finished" => Some("Завершённые"),
            "Downloading" => Some("Загрузка"),
            "Paused" => Some("Пауза"),
            "Completed" => Some("Завершено"),
            "Settings" => Some("Параметры"),
            "Close" => Some("Закрыть"),
            "Open" => Some("Открыть"),
            "Open with..." => Some("Открыть с помощью..."),
            "Open folder" => Some("Открыть папку"),
            "Choose language" => Some("Выберите язык"),
            "Windows' language" => Some("Язык Windows"),
            "Apply" => Some("Применить"),
            _ => None,
        },
        "tr" => match en {
            "File" => Some("Dosya"),
            "Tasks" => Some("Görevler"),
            "View" => Some("Görünüm"),
            "Help" => Some("Yardım"),
            "Add URL" => Some("URL Ekle"),
            "Batch" => Some("Toplu"),
            "Resume" => Some("Devam"),
            "Stop" => Some("Durdur"),
            "Delete" => Some("Sil"),
            "Options" => Some("Seçenekler"),
            "Language..." => Some("Dil..."),
            "All downloads" => Some("Tüm indirmeler"),
            "Unfinished" => Some("Tamamlanmayan"),
            "Finished" => Some("Bitenler"),
            "Downloading" => Some("İndiriliyor"),
            "Paused" => Some("Duraklatıldı"),
            "Completed" => Some("Tamamlandı"),
            "Settings" => Some("Ayarlar"),
            "Close" => Some("Kapat"),
            "Open" => Some("Aç"),
            "Open with..." => Some("Birlikte aç..."),
            "Open folder" => Some("Klasörü aç"),
            "Choose language" => Some("Dil seç"),
            "Windows' language" => Some("Windows dili"),
            "Apply" => Some("Uygula"),
            _ => None,
        },
        "vi" => match en {
            "File" => Some("Tệp"),
            "Tasks" => Some("Tác vụ"),
            "View" => Some("Hiển thị"),
            "Help" => Some("Trợ giúp"),
            "Add URL" => Some("Thêm URL"),
            "Batch" => Some("Hàng loạt"),
            "Resume" => Some("Tiếp tục"),
            "Stop" => Some("Dừng"),
            "Delete" => Some("Xóa"),
            "Options" => Some("Tùy chọn"),
            "Language..." => Some("Ngôn ngữ..."),
            "All downloads" => Some("Tất cả tải xuống"),
            "Unfinished" => Some("Chưa xong"),
            "Finished" => Some("Hoàn tất"),
            "Downloading" => Some("Đang tải"),
            "Paused" => Some("Tạm dừng"),
            "Completed" => Some("Hoàn tất"),
            "Settings" => Some("Cài đặt"),
            "Close" => Some("Đóng"),
            "Open" => Some("Mở"),
            "Open with..." => Some("Mở bằng..."),
            "Open folder" => Some("Mở thư mục"),
            "Choose language" => Some("Chọn ngôn ngữ"),
            "Windows' language" => Some("Ngôn ngữ Windows"),
            "Apply" => Some("Áp dụng"),
            _ => None,
        },
        "hi" => match en {
            "File" => Some("फ़ाइल"),
            "Tasks" => Some("कार्य"),
            "View" => Some("दृश्य"),
            "Help" => Some("सहायता"),
            "Add URL" => Some("URL जोड़ें"),
            "Batch" => Some("बैच"),
            "Resume" => Some("जारी रखें"),
            "Stop" => Some("रोकें"),
            "Delete" => Some("हटाएँ"),
            "Options" => Some("विकल्प"),
            "Language..." => Some("भाषा..."),
            "All downloads" => Some("सभी डाउनलोड"),
            "Unfinished" => Some("अधूरे"),
            "Finished" => Some("पूर्ण"),
            "Downloading" => Some("डाउनलोड हो रहा है"),
            "Paused" => Some("रुका हुआ"),
            "Completed" => Some("पूर्ण"),
            "Settings" => Some("सेटिंग्स"),
            "Close" => Some("बंद करें"),
            "Open" => Some("खोलें"),
            "Open folder" => Some("फ़ोल्डर खोलें"),
            "Choose language" => Some("भाषा चुनें"),
            "Windows' language" => Some("Windows भाषा"),
            "Apply" => Some("लागू करें"),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_windows_languages_and_rtl_flags() {
        assert!(CATALOG.len() >= 86);
        assert!(is_rtl("ar"));
        assert!(is_rtl("he"));
        assert!(is_rtl("fa"));
        assert!(is_rtl("ur"));
        assert!(is_rtl("ug"));
        assert!(!is_rtl("en"));
        assert!(!is_rtl("id"));
    }

    #[test]
    fn locale_numbers_and_sizes_follow_region_rules() {
        assert_eq!(format_grouped_int(10_200, NumberStyle::Plain), "10200");
        assert_eq!(format_grouped_int(10_200, NumberStyle::DotComma), "10.200");
        assert_eq!(
            format_grouped_int(10_200, NumberStyle::SpaceComma),
            "10 200"
        );
        assert_eq!(
            format_grouped_int(100_000, NumberStyle::IndianComma),
            "1,00,000"
        );

        assert_eq!(format_size_locale(1_572_864, "en"), "1.5 MB");
        assert_eq!(format_size_locale(1_572_864, "de"), "1,5 MB");
        assert_eq!(format_size_locale(1_572_864, "id"), "1,5 MB");
        assert_eq!(format_size_locale(1_572_864, "fr"), "1,5 MB");
    }

    #[test]
    fn tsv_catalog_parses_mdm_three_column_format() {
        let raw = "# comment\n#. translator note\n#, machine\nmenu\tAdd &URL\tURL &ekle\ndlg\t&Close\t&Kapat\n";
        let map = parse_tsv_catalog(raw);
        assert_eq!(map.get("Add URL").map(String::as_str), Some("URL ekle"));
        assert_eq!(map.get("Close").map(String::as_str), Some("Kapat"));
    }

    #[test]
    fn builtin_translations_and_english_fallback() {
        assert_eq!(translate("id", "Add URL").as_ref(), "Tambah URL");
        assert_eq!(translate("ar", "All downloads").as_ref(), "كل التنزيلات");
        assert_eq!(
            translate("id", "Untranslated Key Sample").as_ref(),
            "Untranslated Key Sample"
        );
    }
}
