// Art derivation for custom cards: every in-game kind is generated from one
// source artwork per variant, replicating the SIF1-import pipeline that built
// the 7 kinds for all 4,002 imported cards
// (sif1-cards/pipeline/07_prototype.py + 08_convert_all.py + 15_memoria_art.py
// + 17_skillcutin_art.py + 18_blank_art_fallback.py). Uploads are never
// rejected for dimensions: sources and per-kind overrides alike are
// center-crop-covered and Lanczos3-resampled to the exact target sizes, and
// the stored/hashed bytes are always the processed PNG output.
//
// Two source treatments, matching the pipeline's two art classes:
//  * a TRANSPARENT cutout (SIF1 "navi" standee art): the figure (alpha bbox)
//    is placed on the official c_/h_ canvas geometry (fit 1700x1180, centred
//    on 2048x1260) over a blurred+darkened backdrop made from the art itself;
//    m_/sc_/p_/r_ are the pipeline's figure-proportional crops.
//  * OPAQUE artwork: landscape art covers the c_ canvas directly; portrait art
//    gets the blurred-backdrop presentation with the whole image centred.
//    h_ falls back to the c_ content and m_ to its top square, exactly the
//    fallback semantics 18_blank_art_fallback.py shipped for the 74 SIF1
//    cards with no transparent standee.

use image::{imageops, DynamicImage, Rgba, RgbaImage};

// Permissive sanity floor: upscaling small-ish sources is allowed, only
// absurd inputs are refused
pub const MIN_SOURCE_DIM: u32 = 100;

// c_/h_ canvas and the navi fit box (make_full_illust / place_navi)
const CANVAS_W: u32 = 2048;
const CANVAS_H: u32 = 1260;
const FIT_W: f64 = 1700.0;
const FIT_H: f64 = 1180.0;
const BRIGHTNESS: f64 = 0.88;
// GaussianBlur(24) @ 2048x1260 == GaussianBlur(6) @ 512x315 upscaled (the
// pipeline used the same half-res trick for speed)
const BLUR_W: u32 = 512;
const BLUR_H: u32 = 315;
const BLUR_SIGMA: f32 = 6.0;

// m_: head-anchored square of the placed figure (15_memoria_art.py, fitted
// against official pairs at alpha-IoU 0.89-0.94)
const M_SIDE: f64 = 0.78; // of figure height
const M_TOP: f64 = 0.03;  // top edge this far ABOVE the figure top

// sc_: bust crop (17_skillcutin_art.py, fitted at alpha-IoU 0.84-0.92)
const SC_CROP_H: f64 = 0.68;   // of figure height
const SC_CROP_TOP: f64 = -0.055; // of figure height, relative to figure top

pub fn decode_source(name: &str, bytes: &[u8]) -> Result<DynamicImage, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|_| format!("'{}' is not a decodable image (png, jpg and webp work)", name))?;
    if img.width() < MIN_SOURCE_DIM || img.height() < MIN_SOURCE_DIM {
        return Err(format!("'{}' is only {}x{} - at least {}x{} is required", name, img.width(), img.height(), MIN_SOURCE_DIM, MIN_SOURCE_DIM));
    }
    Ok(img)
}

pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut rv = Vec::new();
    DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(rv)
}

fn resize(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    imageops::resize(img, w.max(1), h.max(1), imageops::FilterType::Lanczos3)
}

// Scale-to-cover + centre-crop: the aspect-mismatch treatment for every
// override and for opaque landscape sources (the pipeline's cover())
pub fn cover(img: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let scale = f64::max(tw as f64 / img.width() as f64, th as f64 / img.height() as f64);
    let scaled = resize(img, (img.width() as f64 * scale).round() as u32, (img.height() as f64 * scale).round() as u32);
    let x = (scaled.width() - tw.min(scaled.width())) / 2;
    let y = (scaled.height() - th.min(scaled.height())) / 2;
    imageops::crop_imm(&scaled, x, y, tw, th).to_image()
}

// The bounding box of the non-transparent pixels
fn alpha_bbox(img: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (x, y, px) in img.enumerate_pixels() {
        if px[3] > 0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 == u32::MAX {
        None
    } else {
        Some((x0, y0, x1 + 1, y1 + 1))
    }
}

// A source counts as a transparent cutout (SIF1 navi-style standee) when a
// meaningful share of it is fully transparent background
fn is_cutout(img: &RgbaImage) -> bool {
    let total = (img.width() as u64) * (img.height() as u64);
    let transparent = img.pixels().filter(|px| px[3] < 16).count() as u64;
    transparent * 100 / total.max(1) >= 5
}

// Multiplicative darken (PIL ImageEnhance.Brightness)
fn darken(img: &mut RgbaImage, factor: f64) {
    for px in img.pixels_mut() {
        for c in 0..3 {
            px[c] = (px[c] as f64 * factor).round().min(255.0) as u8;
        }
    }
}

// Crop that pads with transparency outside the source (PIL crop() semantics),
// so figure-proportional crops can reach past the figure
fn crop_pad(src: &RgbaImage, x0: i64, y0: i64, w: u32, h: u32) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    let sx = x0.max(0) as u32;
    let sy = y0.max(0) as u32;
    if sx < src.width() && sy < src.height() {
        let cw = (src.width() - sx).min((x0 + w as i64 - sx as i64).max(0) as u32);
        let ch = (src.height() - sy).min((y0 + h as i64 - sy as i64).max(0) as u32);
        if cw > 0 && ch > 0 {
            let piece = imageops::crop_imm(src, sx, sy, cw, ch).to_image();
            imageops::overlay(&mut canvas, &piece, (sx as i64 - x0).max(0), (sy as i64 - y0).max(0));
        }
    }
    canvas
}

// The blurred + darkened backdrop the c_ presentation puts behind portrait /
// cutout art, made from the art itself (the import used the SIF1 card
// background here; a runtime upload has no separate background, and the
// art-as-its-own-backdrop is the same trick the custom-song jacket blur uses)
fn blurred_backdrop(source: &RgbaImage) -> RgbaImage {
    let mut flat = flatten(source);
    flat = cover(&flat, BLUR_W, BLUR_H);
    flat = imageops::blur(&flat, BLUR_SIGMA);
    darken(&mut flat, BRIGHTNESS);
    resize(&flat, CANVAS_W, CANVAS_H)
}

// Flatten transparency onto the mean colour of the opaque pixels (the
// pipeline flattened onto the tile's centre colour; a cutout's centre may be
// transparent, so the figure's own average is the stable equivalent)
fn flatten(img: &RgbaImage) -> RgbaImage {
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for px in img.pixels() {
        if px[3] >= 128 {
            r += px[0] as u64;
            g += px[1] as u64;
            b += px[2] as u64;
            n += 1;
        }
    }
    let base = if n > 0 {
        Rgba([(r / n) as u8, (g / n) as u8, (b / n) as u8, 255])
    } else {
        Rgba([240, 240, 240, 255])
    };
    let mut canvas = RgbaImage::from_pixel(img.width(), img.height(), base);
    imageops::overlay(&mut canvas, img, 0, 0);
    canvas
}

// place_navi: the figure scaled into the 1700x1180 fit box, and its placement
// on the 2048x1260 canvas
fn place_figure(figure: &RgbaImage) -> (RgbaImage, i64, i64) {
    let scale = f64::min(FIT_H / figure.height() as f64, FIT_W / figure.width() as f64);
    let placed = resize(figure, (figure.width() as f64 * scale).round() as u32, (figure.height() as f64 * scale).round() as u32);
    let x = (CANVAS_W as i64 - placed.width() as i64) / 2;
    let y = (CANVAS_H as i64 - placed.height() as i64) / 2;
    (placed, x, y)
}

// p_: the 136x508 vertical strip through the figure's centre column
fn portrait_strip(figure: &RgbaImage) -> RgbaImage {
    let scaled = resize(figure, ((figure.width() as f64 * 508.0 / figure.height() as f64).round() as u32).max(1), 508);
    let cx = scaled.width() as i64 / 2;
    crop_pad(&scaled, cx - 68, 0, 136, 508)
}

// m_/r_: head-anchored square of the figure (side 0.78 * height, top edge
// 0.03 * height above the figure)
fn head_square(figure: &RgbaImage) -> RgbaImage {
    let h = figure.height() as f64;
    let side = (h * M_SIDE).round().max(1.0) as u32;
    let ox = (figure.width() as i64 - side as i64) / 2;
    let oy = -(h * M_TOP).round() as i64;
    crop_pad(figure, ox, oy, side, side)
}

// sc_: the 2:1 bust crop, bottom edge running off the figure
fn bust_crop(figure: &RgbaImage) -> RgbaImage {
    let fh = figure.height() as f64;
    let ch = (fh * SC_CROP_H).round().max(1.0) as u32;
    let cw = ch * 2;
    let x0 = figure.width() as i64 / 2 - cw as i64 / 2;
    let y0 = (fh * SC_CROP_TOP).round() as i64;
    resize(&crop_pad(figure, x0, y0, cw, ch), 1024, 512)
}

// All 7 card kinds from one source artwork. Every output is at the exact
// official size; the caller overlays any explicit per-kind overrides
pub fn derive_card_art(source: &DynamicImage) -> Vec<(&'static str, RgbaImage)> {
    let rgba = source.to_rgba8();
    let cutout = is_cutout(&rgba).then(|| alpha_bbox(&rgba)).flatten();

    let (c, h, figure) = if let Some((x0, y0, x1, y1)) = cutout {
        // Transparent standee: the official composition
        let figure = imageops::crop_imm(&rgba, x0, y0, x1 - x0, y1 - y0).to_image();
        let (placed, px, py) = place_figure(&figure);
        let mut c = blurred_backdrop(&rgba);
        imageops::overlay(&mut c, &placed, px, py);
        let mut h = RgbaImage::from_pixel(CANVAS_W, CANVAS_H, Rgba([0, 0, 0, 0]));
        imageops::overlay(&mut h, &placed, px, py);
        (c, h, figure)
    } else {
        // Opaque artwork: landscape covers the canvas, portrait gets the
        // blurred-backdrop presentation; h_ falls back to the c_ content
        let c = if rgba.height() > rgba.width() {
            let (placed, px, py) = place_figure(&rgba);
            let mut c = blurred_backdrop(&rgba);
            imageops::overlay(&mut c, &placed, px, py);
            c
        } else {
            cover(&rgba, CANVAS_W, CANVAS_H)
        };
        (c.clone(), c, rgba)
    };

    let t = resize(&c, 512, 315);
    let m = resize(&head_square(&figure), 380, 380);
    let r = resize(&head_square(&figure), 256, 256);
    let sc = bust_crop(&figure);
    let p = portrait_strip(&figure);

    vec![("c", c), ("h", h), ("t", t), ("p", p), ("r", r), ("m", m), ("sc", sc)]
}

// Character icon derived from the portrait when no explicit icon is supplied:
// a top-anchored square for portrait sources (the face is up there), centred
// for landscape ones
pub fn derive_character_icon(portrait: &DynamicImage) -> RgbaImage {
    let rgba = portrait.to_rgba8();
    let side = rgba.width().min(rgba.height());
    let x0 = (rgba.width() - side) / 2;
    let y0 = if rgba.height() > rgba.width() { 0 } else { (rgba.height() - side) / 2 };
    resize(&imageops::crop_imm(&rgba, x0, y0, side, side).to_image(), 230, 230)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cutout_source(w: u32, h: u32) -> DynamicImage {
        // A transparent canvas with an opaque figure occupying the middle band
        let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
        for y in h / 8..h * 7 / 8 {
            for x in w / 3..w * 2 / 3 {
                img.put_pixel(x, y, Rgba([200, 60, 120, 255]));
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    fn opaque_source(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 99, 255])
        }))
    }

    const TARGETS: &[(&str, u32, u32)] = &[
        ("c", 2048, 1260), ("h", 2048, 1260), ("t", 512, 315), ("p", 136, 508),
        ("r", 256, 256), ("m", 380, 380), ("sc", 1024, 512)
    ];

    // Odd-sized sources in -> every kind at its exact official size out,
    // through both the cutout and the opaque path, portrait and landscape
    #[test]
    fn derivation_hits_exact_target_dims() {
        for source in [cutout_source(437, 1013), opaque_source(437, 1013), opaque_source(1900, 700), opaque_source(300, 300)] {
            let derived = derive_card_art(&source);
            assert_eq!(derived.len(), 7);
            for (kind, img) in derived {
                let target = TARGETS.iter().find(|(k, _, _)| *k == kind).unwrap();
                assert_eq!((img.width(), img.height()), (target.1, target.2), "kind {}", kind);
            }
        }
        let icon = derive_character_icon(&opaque_source(431, 617));
        assert_eq!((icon.width(), icon.height()), (230, 230));
    }

    // A transparent standee gets a transparent h_ aligned with c_; an opaque
    // source falls back to h_ == c_ (18_blank_art_fallback semantics)
    #[test]
    fn cutouts_and_opaque_sources_take_their_own_lanes() {
        let derived = derive_card_art(&cutout_source(800, 1000));
        let h = &derived.iter().find(|(k, _)| *k == "h").unwrap().1;
        assert!(h.pixels().any(|px| px[3] == 0), "cutout h_ keeps transparency");
        let c = &derived.iter().find(|(k, _)| *k == "c").unwrap().1;
        assert!(c.pixels().all(|px| px[3] == 255), "c_ backdrop is opaque");

        let derived = derive_card_art(&opaque_source(800, 1000));
        let h = &derived.iter().find(|(k, _)| *k == "h").unwrap().1;
        let c = &derived.iter().find(|(k, _)| *k == "c").unwrap().1;
        assert_eq!(h.as_raw(), c.as_raw(), "opaque h_ falls back to c_");
    }

    #[test]
    fn cover_crops_to_aspect_and_floor_rejects_tiny_sources() {
        let img = opaque_source(1000, 400).to_rgba8();
        let out = cover(&img, 512, 315);
        assert_eq!((out.width(), out.height()), (512, 315));

        let mut tiny = Vec::new();
        opaque_source(32, 32).write_to(&mut std::io::Cursor::new(&mut tiny), image::ImageFormat::Png).unwrap();
        assert!(decode_source("art_00", &tiny).unwrap_err().contains("at least"));
        assert!(decode_source("art_00", b"garbage").unwrap_err().contains("not a decodable image"));
    }
}
