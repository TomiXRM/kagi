//! Settings-window geometry stays in the existing preference store.
#[cfg(not(target_arch = "wasm32"))]
use super::{read_setting, write_setting};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsWindowFrame {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    /// Stable display UUID as hexadecimal; legacy frames have no display.
    pub display_uuid: Option<u128>,
}

#[cfg(not(target_arch = "wasm32"))]
const FRAME_KEY: &str = "settings_window_frame";

#[cfg(any(test, not(target_arch = "wasm32")))]
fn parse_frame(value: &str) -> Option<SettingsWindowFrame> {
    let mut parts = value.split(',');
    let frame = SettingsWindowFrame {
        x: parts.next()?.parse::<f32>().ok()?,
        y: parts.next()?.parse::<f32>().ok()?,
        height: parts.next()?.parse::<f32>().ok()?,
        display_uuid: match parts.next() {
            Some(uuid) if uuid.len() == 32 => Some(u128::from_str_radix(uuid, 16).ok()?),
            Some(_) => return None,
            None => None,
        },
    };
    (parts.next().is_none()
        && frame.x.is_finite()
        && frame.y.is_finite()
        && frame.height.is_finite())
    .then_some(frame)
}

/// Read the frame, migrating the former position/height keys only once.
#[cfg(not(target_arch = "wasm32"))]
pub fn settings_window_frame() -> Option<SettingsWindowFrame> {
    if let Some(value) = read_setting(FRAME_KEY) {
        return parse_frame(&value);
    }
    let position = read_setting("settings_window_position");
    let height = read_setting("settings_window_height");
    let frame = position.as_deref().and_then(|position| {
        let height = height
            .as_deref()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|height| height.is_finite())
            .unwrap_or(720.);
        parse_frame(&format!("{position},{height}"))
    });
    if let Some(frame) = frame {
        write_settings_window_frame(frame);
    }
    if position.is_some() {
        write_setting("settings_window_position", None);
    }
    if height.is_some() {
        write_setting("settings_window_height", None);
    }
    frame
}

/// One changing key lets the sole settings store coalesce a bounds burst.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_settings_window_frame(frame: SettingsWindowFrame) {
    let value = match frame.display_uuid {
        Some(uuid) => format!("{},{},{},{uuid:032x}", frame.x, frame.y, frame.height),
        None => format!("{},{},{}", frame.x, frame.y, frame.height),
    };
    write_setting(FRAME_KEY, Some(&value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_window_frame_requires_three_finite_components() {
        for value in ["", "1,2", "1,2,500,4", "NaN,2,500", "1,inf,500", "1,2,NaN"] {
            assert!(parse_frame(value).is_none(), "{value}");
        }
        assert_eq!(
            parse_frame("-20,30,500"),
            Some(SettingsWindowFrame {
                x: -20.,
                y: 30.,
                height: 500.,
                display_uuid: None,
            })
        );
    }

    #[test]
    fn settings_window_frame_preserves_display_uuid() {
        let frame = parse_frame("20,30,640,000000000000000000000000000000ab").unwrap();
        assert_eq!(frame.display_uuid, Some(0xab));
        assert!(parse_frame("20,30,640,000000000000000000000000000000ab,extra").is_none());
        assert!(parse_frame("20,30,640,gggggggggggggggggggggggggggggggg").is_none());
    }
}
