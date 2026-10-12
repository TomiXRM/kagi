//! A single string-valued preference owns main-window geometry.
use super::{read_setting, write_setting};

#[derive(Clone, Debug, PartialEq)]
pub struct MainWindowFrame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub display: String,
}

fn parse_frame(raw: &str) -> Option<MainWindowFrame> {
    let mut parts = raw.split(',');
    let frame = MainWindowFrame {
        x: parts.next()?.parse().ok()?,
        y: parts.next()?.parse().ok()?,
        width: parts.next()?.parse().ok()?,
        height: parts.next()?.parse().ok()?,
        display: parts.next()?.to_owned(),
    };
    (parts.next().is_none()
        && frame.x.is_finite()
        && frame.y.is_finite()
        && frame.width.is_finite()
        && frame.height.is_finite()
        && frame.width >= 400.
        && frame.height >= 300.)
        .then_some(frame)
}

pub fn main_window_frame() -> Option<MainWindowFrame> {
    parse_frame(&read_setting("window_frame")?)
}

pub fn write_main_window_frame(frame: &MainWindowFrame) {
    write_setting(
        "window_frame",
        Some(&format!(
            "{},{},{},{},{}",
            frame.x, frame.y, frame.width, frame.height, frame.display
        )),
    );
    if read_setting("window_size").is_some() {
        write_setting("window_size", None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_window_frame_rejects_corrupt_geometry() {
        for raw in [
            "",
            "1,2,640,480",
            "NaN,2,640,480,id",
            "1,inf,640,480,id",
            "1,2,inf,480,id",
            "1,2,640,NaN,id",
            "1,2,0,480,id",
            "1,2,640,299,id",
            "1,2,640,480,id,extra",
        ] {
            assert!(parse_frame(raw).is_none(), "{raw}");
        }
        let frame = parse_frame("-20,30,640,480,id").expect("valid signed origin");
        assert_eq!(
            (frame.x, frame.y, frame.width, frame.height),
            (-20., 30., 640., 480.)
        );
        assert_eq!(frame.display, "id");
    }
}
