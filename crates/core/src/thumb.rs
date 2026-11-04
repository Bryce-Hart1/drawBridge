// Claude — Date 11/03/2026
// Thumbnails: a small JPEG of each photo, made from the local file while it
// uploads and stored as `obj/<blob>/thumb`, so a folder of photos can be
// browsed without fetching one full-size image.
//
// Only what the `image` crate decodes here: JPEG, PNG, WebP and GIF (its first
// frame). HEIC thumbnails and video posters come later (M4), drawn by the
// webview, which can decode both.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbImage};

/// The longer side, in pixels.
pub const SIZE: u32 = 256;

const QUALITY: u8 = 80;

/// Bigger than any camera makes. Beyond this, or past the decoder's default
/// memory cap (512 MiB), there is no thumbnail rather than an enormous decode.
const MAX_SIDE: u32 = 20_000;

/// The decoder for `mime`, if it's an image this makes thumbnails of.
fn format(mime: &str) -> Option<ImageFormat> {
    match mime {
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/png" => Some(ImageFormat::Png),
        "image/webp" => Some(ImageFormat::WebP),
        "image/gif" => Some(ImageFormat::Gif),
        _ => None,
    }
}

/// A JPEG of the image at `path`, the right way up and at most SIZE on its
/// longer side. `None` for anything it can't decode: a thumbnail is a nicety,
/// never a reason for an upload to fail.
pub fn make(path: &Path, mime: &str) -> Option<Vec<u8>> {
    let mut reader = ImageReader::with_format(BufReader::new(File::open(path).ok()?), format(mime)?);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    // Phones store photos sideways and say which way is up in EXIF.
    let orientation = decoder.orientation().ok()?;
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    // `thumbnail` would also enlarge a small image; those stay as they are.
    if image.width() > SIZE || image.height() > SIZE {
        image = image.thumbnail(SIZE, SIZE);
    }
    let rgb = flatten(image);
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY).encode_image(&rgb).ok()?;
    Some(out)
}

/// JPEG has no alpha. Transparent pixels go onto white, not black.
fn flatten(image: DynamicImage) -> RgbImage {
    if !image.color().has_alpha() {
        return image.into_rgb8();
    }
    let rgba = image.into_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (from, to) in rgba.pixels().zip(rgb.pixels_mut()) {
        let a = from[3] as u32;
        for c in 0..3 {
            to[c] = ((from[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ColorType, ImageEncoder, Rgba, RgbaImage};

    fn decode(jpeg: &[u8]) -> RgbImage {
        image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).unwrap().into_rgb8()
    }

    fn png(dir: &Path, name: &str, image: &RgbaImage, exif: Option<Vec<u8>>) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut bytes = Vec::new();
        let mut enc = PngEncoder::new(&mut bytes);
        if let Some(exif) = exif {
            enc.set_exif_metadata(exif).unwrap();
        }
        enc.write_image(image.as_raw(), image.width(), image.height(), ColorType::Rgba8.into()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn fits_the_longer_side() {
        let dir = tempfile::tempdir().unwrap();
        let wide = RgbaImage::from_pixel(1000, 500, Rgba([200, 30, 30, 255]));
        let jpeg = make(&png(dir.path(), "wide.png", &wide, None), "image/png").unwrap();
        let thumb = decode(&jpeg);
        assert_eq!((thumb.width(), thumb.height()), (256, 128));

        // Already small: kept at its size.
        let tiny = RgbaImage::from_pixel(40, 30, Rgba([0, 0, 0, 255]));
        let thumb = decode(&make(&png(dir.path(), "tiny.png", &tiny, None), "image/png").unwrap());
        assert_eq!((thumb.width(), thumb.height()), (40, 30));
    }

    #[test]
    fn turns_sideways_photos_upright() {
        let dir = tempfile::tempdir().unwrap();
        // EXIF orientation 6: stored on its side, shown turned 90° clockwise.
        let exif = vec![
            b'M', b'M', 0, 42, 0, 0, 0, 8, // big-endian TIFF header, first IFD at 8
            0, 1, // one entry
            0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, // Orientation, SHORT, 1, value 6
            0, 0, 0, 0, // no next IFD
        ];
        let image = RgbaImage::from_pixel(400, 200, Rgba([10, 10, 10, 255]));
        let thumb = decode(&make(&png(dir.path(), "side.png", &image, Some(exif)), "image/png").unwrap());
        assert_eq!((thumb.width(), thumb.height()), (128, 256));
    }

    #[test]
    fn transparency_goes_white() {
        let dir = tempfile::tempdir().unwrap();
        let clear = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
        let thumb = decode(&make(&png(dir.path(), "clear.png", &clear, None), "image/png").unwrap());
        assert!(thumb.pixels().all(|p| p.0.iter().all(|&c| c > 240)), "{:?}", thumb.get_pixel(0, 0));
    }

    #[test]
    fn anything_else_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.txt");
        std::fs::write(&text, "not an image").unwrap();
        assert!(make(&text, "text/plain").is_none());
        let fake = dir.path().join("fake.jpg");
        std::fs::write(&fake, "not a jpeg either").unwrap();
        assert!(make(&fake, "image/jpeg").is_none());
        assert!(make(&dir.path().join("missing.png"), "image/png").is_none());
        assert!(make(&fake, "image/heic").is_none(), "not a format made here");
    }
}
