//! The RulZ brand: the app icon (`assets/app-icon/`, see its README) at the left of the title bar,
//! where Photoshop shows its "Ps" tile, and the bare "R" mark (`assets/rulz/`) on the Home screen
//! and in About. The PNGs carry their own transparency; each is decoded once per context into a
//! mipmapped texture, so it stays crisp at any size and display scale.

use egui::{Color32, Context, Id, Rect, TextureHandle, TextureOptions, Ui, pos2};
use photocraft_codecs::{ChannelLayout, SampleType};

/// 128 px: sharp at 20 pt on a 3× display, and the mipmaps keep it clean at 1×.
const ICON_PNG: &[u8] = include_bytes!("../../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.photocraft.png");

/// The "R" with a white lower stroke, for dark themes.
const MARK_PNG: &[u8] = include_bytes!("../../../../assets/rulz/mark.png");
/// The "R" in its original red and black, for light themes.
const MARK_LIGHT_PNG: &[u8] = include_bytes!("../../../../assets/rulz/mark-light.png");

/// A PNG's pixels (one transparent pixel if it couldn't be decoded, which a test rules out).
fn decode(png: &[u8]) -> egui::ColorImage {
    let rgba = photocraft_codecs::decode(png).ok().map(|img| img.convert(ChannelLayout::Rgba, SampleType::U8));
    match rgba {
        Some(img) if img.data().len() == img.width() as usize * img.height() as usize * 4 => {
            egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.data())
        }
        _ => egui::ColorImage::filled([1, 1], Color32::TRANSPARENT),
    }
}

/// A brand image's texture, uploaded on first use and kept in the context.
fn texture(ctx: &Context, name: &'static str, png: &[u8]) -> TextureHandle {
    let id = Id::unique(name);
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return tex;
    }
    let options = TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR };
    let tex = ctx.load_texture(name, decode(png), options);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// Where the mark was painted this frame (tests and screenshots).
fn rect_id() -> Id {
    Id::unique("photocraft-brand-mark-rect")
}

/// Paint the mark into `r` (square).
pub fn paint_mark(ui: &Ui, r: Rect) {
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().image(texture(ui.ctx(), "photocraft-brand-mark", ICON_PNG).id(), r, uv, Color32::WHITE);
    ui.ctx().data_mut(|d| d.insert_temp(rect_id(), r));
}

/// Allocate and paint the "R" logo `height` points tall, in the variant for the active theme.
pub fn logo(ui: &mut Ui, height: f32) {
    let dark = crate::theme::Tokens::get(ui.ctx()).dark();
    let (name, png) = if dark { ("photocraft-brand-logo", MARK_PNG) } else { ("photocraft-brand-logo-light", MARK_LIGHT_PNG) };
    let tex = texture(ui.ctx(), name, png);
    let (r, _) = ui.allocate_exact_size(egui::vec2(height * tex.aspect_ratio(), height), egui::Sense::hover());
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    ui.painter().image(tex.id(), r, uv, Color32::WHITE);
}

/// The rect the mark was last painted into.
#[cfg(test)]
pub fn mark_rect(ctx: &Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp(rect_id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes_to_a_rounded_128_px_tile() {
        let img = decode(ICON_PNG);
        assert_eq!(img.size, [128, 128]);
        // Rounded corners: the corner pixel is transparent, the centre opaque.
        assert_eq!(img.pixels[0].a(), 0);
        assert_eq!(img.pixels[64 * 128 + 64].a(), 255);
    }

    #[test]
    fn both_logo_variants_decode_with_transparency() {
        for png in [MARK_PNG, MARK_LIGHT_PNG] {
            let img = decode(png);
            assert!(img.size[0] > 1 && img.size[1] > 1);
            assert!(img.pixels.iter().any(|p| p.a() == 0) && img.pixels.iter().any(|p| p.a() == 255));
        }
    }

    #[test]
    fn the_title_bar_paints_the_icon_from_one_cached_texture() {
        let ctx = Context::default();
        crate::PhotocraftApp::setup_context(&ctx, crate::theme::ThemeKind::ALL[0]);
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        // The system title bar hides the in-app mark (the OS draws its own icon), so paint the
        // app-drawn bar here.
        app.custom_titlebar = true;
        let mut frame = || {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| crate::panels::title_bar(&mut app, ui));
            let uploads = out.textures_delta.set.values().flat_map(|d| d.iter()).filter(|d| d.image.size() == [128, 128]).count();
            out.textures_delta.clear();
            uploads
        };
        assert_eq!(frame(), 1, "uploaded on first use");
        assert_eq!(frame(), 0, "then reused");
        assert!(mark_rect(&ctx).is_some());
    }
}
