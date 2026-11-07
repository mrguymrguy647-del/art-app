//! Help → About Duck Quill: the logo (light or dark, to match the theme),
//! the version number and a line of credits.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Theme};
use quill_engine::image::Image;

/// The logos, shrunk to 700 px so the program file stays small.
const LOGO_LIGHT: &[u8] = include_bytes!("../assets/about-logo-light.png");
const LOGO_DARK: &[u8] = include_bytes!("../assets/about-logo-dark.png");
/// How big the logo is shown, in screen points.
const LOGO_SIZE: f32 = 300.0;

#[derive(Default)]
pub struct About {
    pub open: bool,
    /// Each logo is decoded the first time it's shown, then kept.
    light: Option<TextureHandle>,
    dark: Option<TextureHandle>,
}

impl About {
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let theme = ctx.theme();
        let slot = match theme {
            Theme::Light => &mut self.light,
            Theme::Dark => &mut self.dark,
        };
        let logo = slot.get_or_insert_with(|| {
            let bytes = match theme {
                Theme::Light => LOGO_LIGHT,
                Theme::Dark => LOGO_DARK,
            };
            ctx.load_texture("about-logo", decode(bytes), TextureOptions::LINEAR)
        });
        let logo = egui::load::SizedTexture::new(logo.id(), [LOGO_SIZE, LOGO_SIZE]);
        let mut open = self.open;
        egui::Window::new("About Duck Quill")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.image(logo);
                    ui.heading(format!("Duck Quill {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Draw, paint and animate, with a pen or with a script.");
                    ui.add_space(4.0);
                    ui.small("Made with Rust and egui. Logo type: Lilita One and Fredoka (SIL Open Font License).");
                });
            });
        self.open = open;
    }
}

/// The embedded logo as an egui picture. The logos are part of the
/// program, so if one can't be read the build is broken, not the user's
/// files: a grey square is shown rather than crashing.
fn decode(bytes: &[u8]) -> ColorImage {
    let image = Image::decode(bytes).unwrap_or_else(|_| Image::new(1, 1, [128, 128, 128, 255]));
    let (width, height) = (image.width(), image.height());
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&image.pixel(x, y));
        }
    }
    // The engine keeps pixels premultiplied, as egui wants them here.
    ColorImage::from_rgba_premultiplied([width as usize, height as usize], &rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_logos_are_readable_and_about_700_pixels() {
        for bytes in [LOGO_LIGHT, LOGO_DARK] {
            let image = Image::decode(bytes).expect("the logo should be a PNG");
            assert!((650..=750).contains(&image.width()), "{}", image.width());
            assert!(bytes.len() < 400_000, "keep the program file small");
        }
    }
}
