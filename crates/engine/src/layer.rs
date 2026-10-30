//! Layers: see-through sheets stacked on top of each other.
//!
//! Each layer has its own pixels (a [`Surface`]), a name, a visible/hidden
//! switch and an opacity. The picture you see is all visible layers laid
//! over each other, bottom to top, on the paper.

use crate::surface::Surface;
use crate::tile::Rgba8;

/// A layer's own number. It never changes, even when layers are moved
/// around, so undo can always find the right layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub(crate) u64);

/// How a layer is mixed with the layers below it.
///
/// Only "Normal" exists so far. Later phases add more (Multiply, Screen,
/// ...); they will only need a new arm in [`blend_over`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    /// The layer simply covers what's below, as much as its opacity says.
    #[default]
    Normal,
}

/// The settings of a layer that you can change and undo, all together.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
}

/// One layer.
#[derive(Clone, Debug)]
pub struct Layer {
    id: LayerId,
    name: String,
    visible: bool,
    opacity: f32,
    blend: BlendMode,
    /// The pixels. Painting changes them directly.
    pub surface: Surface,
}

impl Layer {
    /// An empty, visible, fully opaque layer.
    pub(crate) fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            surface: Surface::new(),
        }
    }

    pub fn id(&self) -> LayerId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// 0.0 = invisible, 1.0 = fully opaque.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn blend(&self) -> BlendMode {
        self.blend
    }

    pub fn props(&self) -> LayerProps {
        LayerProps {
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
        }
    }

    /// Changes name, visibility and opacity. If the layer now looks
    /// different, all its tiles are marked as changed, so the screen
    /// updates them.
    pub fn set_props(&mut self, props: LayerProps) {
        let looks_different = props.visible != self.visible || props.opacity != self.opacity;
        self.name = props.name;
        self.visible = props.visible;
        self.opacity = props.opacity.clamp(0.0, 1.0);
        if looks_different {
            self.surface.mark_all_dirty();
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.set_props(LayerProps {
            visible,
            ..self.props()
        });
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.set_props(LayerProps {
            opacity,
            ..self.props()
        });
    }
}

/// Lays `top` (from a layer with the given opacity and blend mode) over
/// `below`. Both pixels are premultiplied.
pub fn blend_over(below: Rgba8, top: Rgba8, opacity: f32, blend: BlendMode) -> Rgba8 {
    match blend {
        BlendMode::Normal => {
            // "Over": the top pixel (made fainter by the layer's opacity),
            // plus as much of the pixel below as still shows through.
            let top_alpha = f32::from(top.a) / 255.0 * opacity;
            let show_through = 1.0 - top_alpha;
            let mix = |t: u8, b: u8| {
                (f32::from(t) * opacity + f32::from(b) * show_through)
                    .round()
                    .min(255.0) as u8
            };
            Rgba8::new(
                mix(top.r, below.r),
                mix(top.g, below.g),
                mix(top.b, below.b),
                mix(top.a, below.a),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const BLUE: Rgba8 = Rgba8::new(0, 0, 255, 255);

    #[test]
    fn an_opaque_top_pixel_covers_what_is_below() {
        assert_eq!(blend_over(RED, BLUE, 1.0, BlendMode::Normal), BLUE);
    }

    #[test]
    fn a_transparent_top_pixel_shows_what_is_below() {
        assert_eq!(
            blend_over(RED, Rgba8::TRANSPARENT, 1.0, BlendMode::Normal),
            RED
        );
        assert_eq!(
            blend_over(
                Rgba8::TRANSPARENT,
                Rgba8::TRANSPARENT,
                1.0,
                BlendMode::Normal
            ),
            Rgba8::TRANSPARENT
        );
    }

    #[test]
    fn layer_opacity_mixes_with_what_is_below() {
        // Blue at 50% layer opacity over red: half and half.
        assert_eq!(
            blend_over(RED, BLUE, 0.5, BlendMode::Normal),
            Rgba8::new(128, 0, 128, 255)
        );
        // Over nothing, it's just half-transparent blue.
        assert_eq!(
            blend_over(Rgba8::TRANSPARENT, BLUE, 0.5, BlendMode::Normal),
            Rgba8::new(0, 0, 128, 128)
        );
    }

    #[test]
    fn changing_how_a_layer_looks_marks_its_tiles_changed() {
        let mut layer = Layer::new(LayerId(1), "Layer 1");
        layer.surface.set_pixel(0, 0, RED);
        layer.surface.set_pixel(1000, 0, RED);
        layer.surface.take_dirty_tiles();

        layer.set_opacity(0.5);
        assert_eq!(layer.surface.take_dirty_tiles().len(), 2);

        // Renaming doesn't change how it looks.
        layer.set_props(LayerProps {
            name: "Sky".into(),
            ..layer.props()
        });
        assert!(layer.surface.take_dirty_tiles().is_empty());
        assert_eq!(layer.name(), "Sky");
    }

    #[test]
    fn opacity_is_kept_between_0_and_1() {
        let mut layer = Layer::new(LayerId(1), "Layer 1");
        layer.set_opacity(1.5);
        assert_eq!(layer.opacity(), 1.0);
        layer.set_opacity(-0.5);
        assert_eq!(layer.opacity(), 0.0);
    }
}
