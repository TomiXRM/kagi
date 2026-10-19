//! Mermaid raster density and logical display geometry, independent of GPUI/I/O.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// UI zoom changes logical size; display backing scale changes only pixel density.
#[derive(Clone, Copy, Debug)]
pub struct MermaidScale {
    zoom: f32,
    raster_scale: f32,
}

impl MermaidScale {
    /// Both inputs come from the validated UI zoom and the window's positive scale.
    pub fn new(zoom: f32, backing_scale: f32) -> Self {
        Self {
            zoom,
            raster_scale: zoom * backing_scale,
        }
    }

    pub fn raster_scale(self) -> f32 {
        self.raster_scale
    }

    /// Convert generated pixels back to zoomed logical units, not device pixels.
    pub fn logical_size(self, pixels: (u32, u32)) -> (f32, f32) {
        let factor = self.zoom / self.raster_scale;
        (pixels.0 as f32 * factor, pixels.1 as f32 * factor)
    }

    /// Only inputs that change the generated PNG belong in its cache identity.
    pub fn cache_key(self, code: &str, dark: bool) -> u64 {
        let mut hash = DefaultHasher::new();
        code.hash(&mut hash);
        dark.hash(&mut hash);
        self.raster_scale.to_bits().hash(&mut hash);
        hash.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::MermaidScale;

    #[test]
    fn retina_pixels_do_not_double_logical_diagram_size() {
        let ordinary = MermaidScale::new(1.0, 1.0);
        let retina = MermaidScale::new(1.0, 2.0);
        assert_eq!(ordinary.logical_size((360, 64)), (360.0, 64.0));
        assert_eq!(retina.logical_size((720, 128)), (360.0, 64.0));
    }

    #[test]
    fn zoom_enlarges_logical_size_at_each_backing_density() {
        for (backing_scale, pixels) in [(1.0, (540, 96)), (1.25, (675, 120)), (2.0, (1080, 192))] {
            let scale = MermaidScale::new(1.5, backing_scale);
            assert_eq!(scale.logical_size(pixels), (540.0, 96.0));
        }
    }

    #[test]
    fn source_theme_and_density_cannot_reuse_another_png() {
        let scale = MermaidScale::new(1.0, 2.0);
        let key = scale.cache_key("A --> B", true);
        assert_ne!(key, scale.cache_key("A --> C", true));
        assert_ne!(key, scale.cache_key("A --> B", false));
        assert_ne!(key, MermaidScale::new(1.0, 1.0).cache_key("A --> B", true));
        assert_ne!(key, MermaidScale::new(1.5, 2.0).cache_key("A --> B", true));
    }

    #[test]
    fn equivalent_density_reuses_pixels_not_logical_size() {
        let zoomed = MermaidScale::new(2.0, 1.0);
        let retina = MermaidScale::new(1.0, 2.0);
        assert_eq!(
            zoomed.cache_key("A --> B", true),
            retina.cache_key("A --> B", true)
        );
        assert_eq!(zoomed.logical_size((720, 128)), (720.0, 128.0));
        assert_eq!(retina.logical_size((720, 128)), (360.0, 64.0));
    }
}
