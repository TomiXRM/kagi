//! The Flower Road Bloom theme.

use crate::{theme::Theme, theme_flower_road::FLOWER_ROAD};

/// An evenly distributed flower wheel for the graph, while keeping Flower
/// Road's existing warm-paper UI intact. The hues are arranged in a
/// three-steps-around-the-wheel order, so adjoining lanes have a 135° hue gap
/// rather than merely following the spectrum.
///
/// Every colour is tuned to at least 3.5:1 against Flower Road's ivory base.
/// That is intentionally more assertive than the original pastel lanes, but
/// still leaves the rest of the theme's soft labels, washes, and chrome alone.
pub const FLOWER_ROAD_BLOOM: Theme = Theme {
    slug: "flower-road-bloom",
    name: "Flower Road Bloom",
    lane_hsl: [
        (0.000, 0.600, 0.609), // Poppy          #d75f5f
        (0.375, 0.600, 0.373), // Leaf green     #269843
        (0.750, 0.600, 0.638), // Iris           #a36bda
        (0.125, 0.600, 0.390), // Marigold       #9f8128
        (0.500, 0.600, 0.361), // Hydrangea      #259393
        (0.875, 0.600, 0.579), // Dahlia         #d453b4
        (0.250, 0.600, 0.361), // Chrysanthemum  #5c9325
        (0.625, 0.600, 0.621), // Cornflower     #6481d8
    ],
    ..FLOWER_ROAD
};
