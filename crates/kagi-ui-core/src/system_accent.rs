//! macOS chrome accent cache. Native preferences are read only at startup/activation.
use std::sync::atomic::{AtomicU8, Ordering};

use crate::i18n::Msg;
#[cfg(not(target_arch = "wasm32"))]
use crate::settings::{read_setting, write_setting};

/// AppleAccentColor: graphite (-1), red (0), orange (1), yellow (2), green
/// (3), blue (4), purple (5), pink (6). Absent/multicolour uses blue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SystemAccent {
    Graphite,
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    /// Missing/multicolour or unrecognised preference: application's blue.
    Default,
}

impl SystemAccent {
    pub fn from_default(value: Option<i32>) -> Self {
        match value {
            Some(-1) => Self::Graphite,
            Some(0) => Self::Red,
            Some(1) => Self::Orange,
            Some(2) => Self::Yellow,
            Some(3) => Self::Green,
            Some(4) => Self::Blue,
            Some(5) => Self::Purple,
            Some(6) => Self::Pink,
            _ => Self::Default,
        }
    }

    pub fn detected_label(self) -> &'static str {
        match self {
            Self::Graphite => Msg::SettingsAccentGraphite,
            Self::Red => Msg::SettingsAccentRed,
            Self::Orange => Msg::SettingsAccentOrange,
            Self::Yellow => Msg::SettingsAccentYellow,
            Self::Green => Msg::SettingsAccentGreen,
            Self::Blue => Msg::SettingsAccentBlue,
            Self::Purple => Msg::SettingsAccentPurple,
            Self::Pink => Msg::SettingsAccentPink,
            Self::Default => Msg::SettingsAccentDefault,
        }
        .t()
    }

    pub fn rgb(self, dark: bool) -> u32 {
        let palette = if dark {
            &crate::theme_apple_dark::SYSTEM_ACCENTS
        } else {
            &crate::theme_apple_light::SYSTEM_ACCENTS
        };
        palette[self.palette_index()]
    }

    pub(crate) const fn palette_index(self) -> usize {
        match self {
            Self::Default => Self::Blue as usize,
            _ => self as usize,
        }
    }
}

static DETECTED: AtomicU8 = AtomicU8::new(SystemAccent::Blue as u8);
// 0 = theme-specific default, 1 = explicit System, 2 = explicit Theme accent.
static ACCENT_SOURCE: AtomicU8 = AtomicU8::new(0);
#[cfg(any(test, feature = "gui-e2e"))]
thread_local! {
    static INJECTED: std::cell::Cell<Option<(SystemAccent, Option<bool>)>> = const { std::cell::Cell::new(None) };
}

pub fn detected_system_accent() -> SystemAccent {
    #[cfg(any(test, feature = "gui-e2e"))]
    if let Some((accent, _)) = INJECTED.with(std::cell::Cell::get) {
        return accent;
    }
    SystemAccent::from_default(Some(DETECTED.load(Ordering::Relaxed) as i32 - 1))
}

pub fn follows_system_accent() -> bool {
    follows_system_accent_for(crate::theme::theme().dark)
}

pub(crate) fn follows_system_accent_for(dark: bool) -> bool {
    #[cfg(any(test, feature = "gui-e2e"))]
    if let Some((_, follow)) = INJECTED.with(std::cell::Cell::get) {
        return follow.unwrap_or(!dark);
    }
    match ACCENT_SOURCE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => !dark,
    }
}

#[cfg(any(test, not(target_arch = "wasm32")))]
fn source_from_setting(value: Option<&str>) -> Option<bool> {
    match value {
        Some("system") => Some(true),
        Some("theme" | "blue") => Some(false),
        _ => None,
    }
}

/// Initialize host-loaded accent state without querying OS preferences or storage.
/// `None` follows the preset default; `Some` explicitly selects system/theme.
pub fn init_loaded_accent(accent: SystemAccent, follow: Option<bool>) {
    DETECTED.store(accent as u8, Ordering::Relaxed);
    ACCENT_SOURCE.store(
        match follow {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        },
        Ordering::Relaxed,
    );
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_follows_system_accent(follow: bool) {
    #[cfg(any(test, feature = "gui-e2e"))]
    let injected = INJECTED.with(|slot| {
        if let Some((accent, _)) = slot.get() {
            slot.set(Some((accent, Some(follow))));
            true
        } else {
            false
        }
    });
    #[cfg(not(any(test, feature = "gui-e2e")))]
    let injected = false;
    if !injected {
        ACCENT_SOURCE.store(if follow { 1 } else { 2 }, Ordering::Relaxed);
    }
    write_setting(
        "accent_source",
        Some(if follow { "system" } else { "theme" }),
    );
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn init() {
    let source = read_setting("accent_source");
    let legacy = read_setting("accent");
    let follow = source_from_setting(source.as_deref().or(legacy.as_deref()));
    ACCENT_SOURCE.store(
        match follow {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        },
        Ordering::Relaxed,
    );
    if source.as_deref() == Some("blue") || (source.is_none() && legacy.is_some()) {
        write_setting(
            "accent_source",
            follow.map(|follow| if follow { "system" } else { "theme" }),
        );
    }
    if legacy.is_some() {
        write_setting("accent", None);
    }
    refresh_system_accent();
}

/// Interpret an owned native preference value; `None` represents a null result.
#[cfg(target_os = "macos")]
fn from_cf_value(value: Option<core_foundation::base::CFType>) -> SystemAccent {
    use core_foundation::{base::TCFType, number::CFNumber, string::CFString};
    let value = value.and_then(|value| {
        if value.instance_of::<CFNumber>() {
            value.downcast_into::<CFNumber>()?.to_i32()
        } else {
            let string = value.downcast_into::<CFString>()?;
            // Borrow native UTF-8 storage when available; otherwise CFString's
            // conversion owns it. No conversion happens in the render path.
            let text: std::borrow::Cow<'_, str> = (&string).into();
            text.trim().parse::<i32>().ok()
        }
    });
    SystemAccent::from_default(value)
}

/// Read-only, in-process global preference query, only at startup/activation.
/// Multicolour (missing or unsupported value) follows the application's blue.
#[cfg(not(target_arch = "wasm32"))]
pub fn refresh_system_accent() -> bool {
    #[cfg(any(test, feature = "gui-e2e"))]
    if INJECTED.with(std::cell::Cell::get).is_some() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        use core_foundation::base::{CFType, TCFType};
        use core_foundation::string::CFString;
        use core_foundation_sys::preferences::{
            kCFPreferencesAnyApplication, CFPreferencesCopyAppValue,
        };
        thread_local! {
            static ACCENT_KEY: CFString = CFString::from_static_string("AppleAccentColor");
        }
        let value = ACCENT_KEY.with(|key| {
            // SAFETY: key and the global domain constant are valid CFStrings.
            // CopyAppValue returns an owned CF property list or null. The owned
            // CFType releases it on every path; downcast checks its type before
            // interpreting it as a number or string. This API never writes preferences.
            let raw = unsafe {
                CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), kCFPreferencesAnyApplication)
            };
            from_cf_value(if raw.is_null() {
                None
            } else {
                Some(unsafe { CFType::wrap_under_create_rule(raw) })
            })
        });
        let next = value as u8;
        DETECTED.swap(next, Ordering::Relaxed) != next
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Scoped in-memory injection; never reads or writes the person's defaults.
#[cfg(any(test, feature = "gui-e2e"))]
pub struct AccentInjection(
    Option<(SystemAccent, Option<bool>)>,
    std::marker::PhantomData<std::rc::Rc<()>>,
);
#[cfg(any(test, feature = "gui-e2e"))]
impl AccentInjection {
    /// `None` exercises the preset default; `Some` is an explicit source choice.
    pub fn new(accent: SystemAccent, follow: Option<bool>) -> Self {
        Self(
            INJECTED.with(|slot| slot.replace(Some((accent, follow)))),
            std::marker::PhantomData,
        )
    }
}
#[cfg(any(test, feature = "gui-e2e"))]
impl Drop for AccentInjection {
    fn drop(&mut self) {
        INJECTED.with(|slot| slot.set(self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_source_accepts_explicit_modes_and_legacy_blue() {
        assert_eq!(source_from_setting(None), None);
        assert_eq!(source_from_setting(Some("system")), Some(true));
        assert_eq!(source_from_setting(Some("theme")), Some(false));
        assert_eq!(source_from_setting(Some("blue")), Some(false));
        assert_eq!(source_from_setting(Some("unsupported")), None);
        let _default = AccentInjection::new(SystemAccent::Blue, None);
        assert!(follows_system_accent_for(false), "Light defaults to System");
        assert!(!follows_system_accent_for(true), "Dark defaults to Theme");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_preference_numbers_strings_null_and_other_types() {
        use core_foundation::{
            base::TCFType, boolean::CFBoolean, number::CFNumber, string::CFString,
        };
        for value in -1..=6 {
            assert_eq!(
                from_cf_value(Some(CFNumber::from(value).into_CFType())),
                SystemAccent::from_default(Some(value)),
            );
        }
        assert_eq!(
            from_cf_value(Some(CFString::from_static_string("1").into_CFType())),
            SystemAccent::Orange,
            "AppKit accepts a string-stored AppleAccentColor",
        );
        for value in -1..=6 {
            let text = format!(" \t{value}\n ");
            assert_eq!(
                from_cf_value(Some(CFString::new(&text).into_CFType())),
                SystemAccent::from_default(Some(value)),
            );
        }
        assert_eq!(from_cf_value(None), SystemAccent::Default);
        assert_eq!(
            from_cf_value(Some(CFBoolean::true_value().into_CFType())),
            SystemAccent::Default,
        );
        for text in ["", "not-a-number", "99999999999999999999", "7"] {
            assert_eq!(
                from_cf_value(Some(CFString::from_static_string(text).into_CFType())),
                SystemAccent::Default,
            );
        }
    }

    #[test]
    fn all_defaults_map_to_standard_light_and_dark_accents() {
        let light = [
            0x8e8e93, 0xff3b30, 0xff9500, 0xffcc00, 0x34c759, 0x007aff, 0xaf52de, 0xff2d55,
        ];
        let dark = [
            0x98989d, 0xff453a, 0xff9f0a, 0xffd60a, 0x30d158, 0x0a84ff, 0xbf5af2, 0xff375f,
        ];
        for (index, value) in (-1..=6).enumerate() {
            let accent = SystemAccent::from_default(Some(value));
            assert_eq!(accent.rgb(false), light[index]);
            assert_eq!(accent.rgb(true), dark[index]);
        }
        for unknown in [None, Some(-2), Some(7), Some(99)] {
            let accent = SystemAccent::from_default(unknown);
            assert_eq!(accent, SystemAccent::Default);
            assert_eq!(accent.rgb(false), SystemAccent::Blue.rgb(false));
            assert_eq!(accent.rgb(true), SystemAccent::Blue.rgb(true));
        }
    }
}
