//! The Flower Road Vivid theme.

use crate::{theme::Theme, theme_flower_road::FLOWER_ROAD};

/// A stronger rendering of the Bloom theme. It keeps the same eight flower
/// hues and spacing, then raises saturation to bring the lane contrast and
/// categorical separation closer to Apple Dark's vivid graph palette.
pub const FLOWER_ROAD_VIVID: Theme = Theme {
    slug: "flower-road-vivid",
    name: "Flower Road Vivid",
    lane_hsl: [
        (0.000, 0.700, 0.613), // Poppy          #e15757
        (0.375, 0.700, 0.354), // Leaf green     #1b993b
        (0.750, 0.700, 0.650), // Iris           #a667e4
        (0.125, 0.700, 0.374), // Marigold       #a2811d
        (0.500, 0.700, 0.340), // Hydrangea      #1a9393
        (0.875, 0.700, 0.574), // Dahlia         #de46b8
        (0.250, 0.700, 0.342), // Chrysanthemum  #57941a
        (0.625, 0.700, 0.632), // Cornflower     #5f80e3
    ],
    ..FLOWER_ROAD
};
