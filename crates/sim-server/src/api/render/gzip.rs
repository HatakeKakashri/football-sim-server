//! The gzip coding of the stored segment bodies (server-API ticket 04).
//!
//! Segments are compressed as they are rendered, at the fixed level
//! [`GZIP_LEVEL`]. The compressed bytes are kept, and the uncompressed
//! body is published beside them (see [`super::PublishedSegment`]), so
//! a client that does not accept gzip is served the decompressed bytes.
//! The decompressor lives here next to the compressor.
//!
//! The level is fixed so the stored bytes are reproducible: the same
//! body always compresses to the same bytes, on any machine, with any
//! zlib. The compressed bytes are *not* part of a segment's identity
//! (that is the uncompressed SHA-256), but reproducible bytes keep the
//! stored match cacheable and comparable.

use std::fmt;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

/// The gzip level the stored segment bodies are compressed with.
///
/// Level 6 is gzip's own default: the size difference against level 9
/// is a few percent on these bodies, and the CPU cost of level 9 is
/// paid on every render. The level is a constant, never a knob, so
/// that two renders of the same match produce the same stored bytes.
pub const GZIP_LEVEL: u32 = 6;

/// A failure to decompress a stored segment body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GzipError {
    /// What went wrong, in human-readable form.
    message: String,
}

impl GzipError {
    /// Build an error from a message.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GzipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GzipError {}

/// Compress bytes with gzip at the fixed level.
///
/// # Panics
///
/// Panics if the encoder cannot be written to or finished, which can
/// only happen on an allocation failure: the encoder writes into a
/// `Vec`, so there is no I/O error to handle.
#[must_use]
pub fn compress(bytes: &[u8]) -> Vec<u8> {
    compress_at_level(bytes, GZIP_LEVEL)
}

/// Compress bytes with gzip at an explicit level.
///
/// The level is a parameter so a test can prove that a segment's
/// identity does not depend on the compressed bytes.
#[must_use]
pub fn compress_at_level(bytes: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
    encoder
        .write_all(bytes)
        .expect("invariant: writing gzip output into a Vec cannot fail");
    encoder
        .finish()
        .expect("invariant: finishing a gzip encoder into a Vec cannot fail")
}

/// Decompress gzip bytes.
///
/// # Errors
///
/// Returns a [`GzipError`] when the input is not a valid gzip stream
/// or the stream is truncated.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>, GzipError> {
    let mut decoder = GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|error| GzipError::new(format!("could not decompress the segment: {error}")))?;
    Ok(out)
}
