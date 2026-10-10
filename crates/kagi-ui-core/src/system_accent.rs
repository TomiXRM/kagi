//! macOS chrome accent cache. Native preferences are read only at startup/activation.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::i18n::Msg;
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
static FOLLOW_SYSTEM: AtomicBool = AtomicBool::new(true);
#[cfg(any(test, feature = "gui-e2e"))]
thread_local! {
    static INJECTED: std::cell::Cell<Option<(SystemAccent, bool)>> = const { std::cell::Cell::new(None) };
}

pub fn detected_system_accent() -> SystemAccent {
    #[cfg(any(test, feature = "gui-e2e"))]
    if let Some((accent, _)) = INJECTED.with(std::cell::Cell::get) {
        return accent;
    }
    SystemAccent::from_default(Some(DETECTED.load(Ordering::Relaxed) as i32 - 1))
}

pub fn follows_system_accent() -> bool {
    #[cfg(any(test, feature = "gui-e2e"))]
    if let Some((_, follow)) = INJECTED.with(std::cell::Cell::get) {
        return follow;
    }
    FOLLOW_SYSTEM.load(Ordering::Relaxed)
}

pub fn set_follows_system_accent(follow: bool) {
    #[cfg(any(test, feature = "gui-e2e"))]
    let injected = INJECTED.with(|slot| {
        if let Some((accent, _)) = slot.get() {
            slot.set(Some((accent, follow)));
            true
        } else {
            false
        }
    });
    #[cfg(not(any(test, feature = "gui-e2e")))]
    let injected = false;
    if !injected {
        FOLLOW_SYSTEM.store(follow, Ordering::Relaxed);
    }
    write_setting("accent", Some(if follow { "system" } else { "blue" }));
}

pub(crate) fn init() {
    FOLLOW_SYSTEM.store(
        read_setting("accent").as_deref() != Some("blue"),
        Ordering::Relaxed,
    );
    refresh_system_accent();
}

/// Read-only, in-process global preference query, only at startup/activation.
/// Multicolour (missing or unsupported value) follows the application's blue.
pub fn refresh_system_accent() -> bool {
    #[cfg(any(test, feature = "gui-e2e"))]
    if INJECTED.with(std::cell::Cell::get).is_some() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        use core_foundation::base::{CFType, TCFType};
        use core_foundation::number::CFNumber;
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
            // interpreting it as a number. This API never writes preferences.
            let raw = unsafe {
                CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), kCFPreferencesAnyApplication)
            };
            if raw.is_null() {
                return None;
            }
            let value = unsafe { CFType::wrap_under_create_rule(raw) };
            value.downcast_into::<CFNumber>()?.to_i32()
        });
        let next = SystemAccent::from_default(value) as u8;
        return DETECTED.swap(next, Ordering::Relaxed) != next;
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(target_os = "macos")]
pub(crate) fn resolved_accent() -> SystemAccent {
    if follows_system_accent() {
        detected_system_accent()
    } else {
        SystemAccent::Blue
    }
}

/// Scoped in-memory injection; never reads or writes the person's defaults.
#[cfg(any(test, feature = "gui-e2e"))]
pub struct AccentInjection(
    Option<(SystemAccent, bool)>,
    std::marker::PhantomData<std::rc::Rc<()>>,
);
#[cfg(any(test, feature = "gui-e2e"))]
impl AccentInjection {
    pub fn new(accent: SystemAccent) -> Self {
        Self(
            INJECTED.with(|slot| slot.replace(Some((accent, true)))),
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
