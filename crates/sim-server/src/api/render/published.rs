//! The published bytes of a rendered match (server-API ticket 01).
//!
//! A rendered match is published as immutable bytes: the manifest's
//! answer body and every segment's body, produced once at render time
//! and never derived again. A served manifest is a copy of those bytes
//! instead of a value re-serialised per request, and a segment is
//! found by its number instead of by scanning a list.
//!
//! # The manifest body
//!
//! The manifest's answer body is the manifest's own JSON with the
//! `state` field appended, so the published bytes are byte-identical
//! to the body the manifest route serves. The body type lives here,
//! next to the manifest it flattens, so the wire format of a manifest
//! body lives in one place.
//!
//! The published match also carries the build id: the digest every
//! published URL names. A caller that serves the bytes needs it for
//! the manifest's validator and for the build-id check of a segment
//! URL.
//!
//! # The segment bodies
//!
//! A published segment carries its body — the uncompressed JSON, whose
//! SHA-256 is the hash the manifest publishes — and the gzip coding of
//! it. Both are shared between clones and never mutated, so serving a
//! segment costs no copy of the underlying buffer. The manifest's own
//! row for the segment stays in the manifest: the published match
//! publishes bytes, not a second copy of the manifest.
//!
//! # Memory
//!
//! Publishing keeps the uncompressed bodies of the whole match, so the
//! render step's memory is the match's bodies rather than one
//! segment's. For a full match they are about twenty times the size of
//! the compressed ones (137 MB against 6.5 MB for `m-0001`), and that
//! is the trade the published bytes buy: the bodies are the match's
//! content, and a client that asks for one gets a copy of bytes that
//! already exist instead of a body derived again.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use super::{Manifest, StoredSegment};

/// The `state` a published manifest body carries.
///
/// The body is only ever the answer of a ready match, so the state is
/// part of the body's shape rather than a choice the caller makes.
pub const STATE_READY: &str = "ready";

/// The body of a ready match's manifest.
///
/// The manifest is flattened into the body and the `state` field comes
/// last, so the body is the manifest's own JSON with the state
/// appended: the bytes this type produces are the bytes the manifest
/// route serves.
///
/// The body borrows the manifest: publishing a match copies the
/// manifest's bytes once, not the manifest itself.
#[derive(Debug, Serialize)]
pub struct ManifestBody<'a> {
    /// The manifest, flattened into the body.
    #[serde(flatten)]
    manifest: &'a Manifest,
    /// Always [`STATE_READY`].
    state: &'static str,
}

impl<'a> ManifestBody<'a> {
    /// The body of a ready match's manifest.
    #[must_use]
    pub const fn ready(manifest: &'a Manifest) -> Self {
        Self {
            manifest,
            state: STATE_READY,
        }
    }
}

/// One published segment: the bytes it serves.
///
/// The body and its gzip coding are shared between clones and never
/// mutated, so serving a segment from several places costs one copy
/// of the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedSegment {
    /// The segment's body: the uncompressed JSON.
    ///
    /// Its SHA-256 is the hash the manifest publishes.
    pub body: Arc<[u8]>,
    /// The gzip coding of the body.
    pub gzipped_body: Arc<[u8]>,
}

/// A rendered match published as immutable bytes.
///
/// The published match owns the bytes the routes serve: the manifest's
/// answer body and every segment's body, indexed by segment number.
/// The bytes are produced once, at render time, and are never derived
/// again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedMatch {
    /// The manifest's answer body, as it goes on the wire.
    manifest_body: Arc<[u8]>,
    /// The build id of the published match.
    ///
    /// The digest every published URL of this match carries.
    build_id: String,
    /// The segments, indexed by segment number.
    segments: BTreeMap<u32, PublishedSegment>,
}

impl PublishedMatch {
    /// Publish a rendered match: the manifest's answer body and every
    /// segment's body, indexed by segment number.
    ///
    /// The index is built from the rendered segments, so it holds
    /// exactly the numbers the manifest lists.
    ///
    /// # Panics
    ///
    /// Panics if the manifest body cannot be serialised, which cannot
    /// happen for a manifest this crate built.
    pub(super) fn publish(manifest: &Manifest, stored: &[StoredSegment]) -> Self {
        let body = serde_json::to_vec(&ManifestBody::ready(manifest))
            .expect("invariant: the manifest body is JSON-serialisable");
        let segments = stored
            .iter()
            .map(|segment| {
                (
                    segment.range.number,
                    PublishedSegment {
                        body: Arc::clone(&segment.uncompressed_body),
                        gzipped_body: Arc::clone(&segment.body),
                    },
                )
            })
            .collect();
        Self {
            manifest_body: Arc::from(body),
            build_id: manifest.build_id.clone(),
            segments,
        }
    }

    /// The manifest's answer body, as shared bytes.
    ///
    /// These are the bytes the manifest route serves: they are
    /// produced once, at render time, and are never derived again. The
    /// buffer is shared, so serving it costs no copy.
    #[must_use]
    pub fn manifest_body(&self) -> Arc<[u8]> {
        Arc::clone(&self.manifest_body)
    }

    /// The build id of the published match.
    ///
    /// The build id is a digest of the rendered content, so it names
    /// the bytes behind every published URL of this match.
    #[must_use]
    pub fn build_id(&self) -> &str {
        &self.build_id
    }

    /// The published segment with this number, if the match has one.
    ///
    /// The manifest lists the segments from zero, so a number the
    /// manifest does not list names nothing.
    #[must_use]
    pub fn segment(&self, number: u32) -> Option<&PublishedSegment> {
        self.segments.get(&number)
    }

    /// The number of published segments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Whether the match published no segment at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}
