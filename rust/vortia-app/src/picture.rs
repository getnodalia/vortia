//! Saving what the viewer shows as a PNG in the case, next to the other things a run writes
//! (`postProcessing/pictures/`). The window is captured and cropped to the picture, so what is
//! saved is what is on the screen, with the colours and the section as they are.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, ColorImage, Rect};

use crate::i18n::{tr, trf};

/// The most pixels a picture may have; a window is far below it.
const MAX_PIXELS: usize = 64_000_000;

/// Where the picture of `what` (a field, or the patches) at `time` is written in the case.
pub fn path_for(case: &Path, what: &str, time: &str) -> PathBuf {
    // A field or a time never carries a separator, but they come from a folder listing.
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    case.join("postProcessing").join("pictures").join(format!(
        "{}_{}.png",
        clean(what),
        clean(time)
    ))
}

/// The part of `shot` that is `region` (in points), as a PNG.
pub fn crop_png(shot: &ColorImage, region: Rect, pixels_per_point: f32) -> Result<Vec<u8>, String> {
    let [width, height] = shot.size;
    if width == 0 || height == 0 || width.saturating_mul(height) > MAX_PIXELS {
        return Err(tr("The picture could not be taken.").to_owned());
    }
    // Pixels, clamped to the screenshot: egui's own crop panics on a region outside it.
    let to_pixel = |points: f32, limit: usize| -> usize {
        let pixel = points * pixels_per_point;
        if pixel.is_nan() || pixel <= 0.0 {
            0
        } else {
            (pixel as usize).min(limit)
        }
    };
    let (x0, x1) = (to_pixel(region.min.x, width), to_pixel(region.max.x, width));
    let (y0, y1) = (
        to_pixel(region.min.y, height),
        to_pixel(region.max.y, height),
    );
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        return Err(tr("The picture could not be taken.").to_owned());
    }
    let mut bytes = Vec::with_capacity(w * h * 4);
    for row in y0..y1 {
        let start = row * width + x0;
        let Some(pixels) = shot.pixels.get(start..start + w) else {
            return Err(tr("The picture could not be taken.").to_owned());
        };
        bytes.extend(pixels.iter().flat_map(|c| c.to_array()));
    }
    let image = image::RgbaImage::from_raw(
        u32::try_from(w).map_err(|e| e.to_string())?,
        u32::try_from(h).map_err(|e| e.to_string())?,
        bytes,
    )
    .ok_or_else(|| tr("The picture could not be taken.").to_owned())?;
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Asks the window for a screenshot, crops it to `region` and writes it to `path`. The result
/// (whether it worked, and what to tell the user) arrives on the channel.
pub fn request(
    ctx: &egui::Context,
    region: Rect,
    path: PathBuf,
    case: PathBuf,
) -> Receiver<(bool, String)> {
    let (tx, rx) = channel();
    let ppp = ctx.pixels_per_point();
    let wake = ctx.clone();
    ctx.request_screenshot(move |shot: Arc<ColorImage>| {
        let done = crop_png(&shot, region, ppp)
            .and_then(|png| vortia_foamfile::write::atomic(&path, &png).map_err(|e| e.to_string()));
        let message = match done {
            Ok(()) => (
                true,
                trf(
                    "Saved to {}.",
                    &[&path.strip_prefix(&case).unwrap_or(&path).display()],
                ),
            ),
            Err(e) => (false, e),
        };
        let _ = tx.send(message);
        wake.request_repaint();
    });
    ctx.request_repaint();
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, pos2};

    #[test]
    fn the_path_stays_inside_the_pictures_folder_whatever_the_names() {
        let path = path_for(Path::new("/c"), "../../etc/p", "0.5/x");
        assert_eq!(path.parent(), Some(Path::new("/c/postProcessing/pictures")));
        assert_eq!(path.file_name().unwrap(), ".._.._etc_p_0.5_x.png");
    }

    #[test]
    fn a_region_of_the_screenshot_becomes_a_png_of_that_size() {
        let shot = ColorImage::filled([40, 20], Color32::from_rgb(10, 20, 30));
        let png = crop_png(
            &shot,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 5.0)),
            2.0,
        )
        .expect("a png");
        let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .expect("a png that reads back");
        assert_eq!((decoded.width(), decoded.height()), (20, 10));
    }

    #[test]
    fn an_empty_or_outside_region_is_an_error_not_a_panic() {
        let shot = ColorImage::filled([8, 8], Color32::WHITE);
        let outside = Rect::from_min_max(pos2(100.0, 100.0), pos2(120.0, 120.0));
        assert!(crop_png(&shot, outside, 1.0).is_err());
        let empty = Rect::from_min_max(pos2(2.0, 2.0), pos2(2.0, 2.0));
        assert!(crop_png(&shot, empty, 1.0).is_err());
        assert!(crop_png(&ColorImage::filled([0, 0], Color32::WHITE), outside, 1.0).is_err());
        let nan = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(4.0, f32::INFINITY));
        let _ = crop_png(&shot, nan, f32::NAN);
    }
}
