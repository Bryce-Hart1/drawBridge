// Claude — Date 11/03/2026
// A file's MIME type and swiftSearch kind, from its extension. The MIME type
// goes into the blob's manifest, and the stream server (M4) serves content
// with it. The kind becomes `X-SwiftSearch-Type` on every chunk.
//
// The image, video and text extensions match swiftSearch's own table (USE.md,
// "Value types"), so a chunk's kind agrees with what swiftSearch would have
// guessed from the same name. swiftSearch has no audio kind, so audio is `bin`.

use std::path::Path;

use crate::remote::Kind;

pub fn detect(path: &Path) -> (&'static str, Kind) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let (mime, kind) = match ext.as_str() {
        "jpg" | "jpeg" => ("image/jpeg", Kind::Img),
        "png" => ("image/png", Kind::Img),
        "gif" => ("image/gif", Kind::Img),
        "webp" => ("image/webp", Kind::Img),
        "avif" => ("image/avif", Kind::Img),
        "bmp" => ("image/bmp", Kind::Img),
        "tif" | "tiff" => ("image/tiff", Kind::Img),
        "heic" => ("image/heic", Kind::Img),
        "heif" => ("image/heif", Kind::Img),
        "svg" => ("image/svg+xml", Kind::Img),

        "mp4" => ("video/mp4", Kind::Video),
        "m4v" => ("video/x-m4v", Kind::Video),
        "webm" => ("video/webm", Kind::Video),
        "mov" => ("video/quicktime", Kind::Video),
        "mkv" => ("video/x-matroska", Kind::Video),
        "ogv" => ("video/ogg", Kind::Video),
        "avi" => ("video/x-msvideo", Kind::Video),
        "ts" => ("video/mp2t", Kind::Video),

        "mp3" => ("audio/mpeg", Kind::Bin),
        "m4a" => ("audio/mp4", Kind::Bin),
        "aac" => ("audio/aac", Kind::Bin),
        "flac" => ("audio/flac", Kind::Bin),
        "wav" => ("audio/wav", Kind::Bin),
        "ogg" | "oga" => ("audio/ogg", Kind::Bin),
        "opus" => ("audio/opus", Kind::Bin),

        "txt" | "log" => ("text/plain; charset=utf-8", Kind::Str),
        "md" | "markdown" => ("text/markdown; charset=utf-8", Kind::Str),
        "csv" => ("text/csv; charset=utf-8", Kind::Str),
        "json" => ("application/json", Kind::Str),
        // Recorded as what it is. Whatever serves it to a webview must not
        // serve it as HTML: it would run script in the app's origin.
        "html" | "htm" => ("text/html; charset=utf-8", Kind::Str),

        "pdf" => ("application/pdf", Kind::Bin),
        "zip" => ("application/zip", Kind::Bin),
        _ => ("application/octet-stream", Kind::Bin),
    };
    (mime, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_extension() {
        assert_eq!(detect(Path::new("a/beach.JPG")), ("image/jpeg", Kind::Img));
        assert_eq!(detect(Path::new("intro.mp4")), ("video/mp4", Kind::Video));
        assert_eq!(detect(Path::new("song.mp3")), ("audio/mpeg", Kind::Bin));
        assert_eq!(detect(Path::new("notes.md")).1, Kind::Str);
        assert_eq!(detect(Path::new("Makefile")), ("application/octet-stream", Kind::Bin));
        assert_eq!(detect(Path::new(".bashrc")), ("application/octet-stream", Kind::Bin));
        assert_eq!(detect(Path::new("archive.tar.gz")), ("application/octet-stream", Kind::Bin));
    }
}
