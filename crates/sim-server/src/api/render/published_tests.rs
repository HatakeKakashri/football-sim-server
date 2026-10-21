//! Tests for the published bytes of a rendered match (server-API
//! ticket 01).
//!
//! The published bytes are what the routes serve, so they are pinned
//! here: the manifest's answer body is byte-identical to the body the
//! manifest route builds, every segment's body hashes to the hash the
//! manifest publishes, and the index covers exactly the manifest's
//! segments.

use std::fmt;

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::digest::{hex, sha256};
use super::{ManifestBody, render_match};
use crate::api::render_doubles::{fixture, hand_built_match};

/// The manifest's own fields, in the order the manifest publishes them.
///
/// The order is part of the wire format: the published body's keys
/// are these, followed by the `state` field.
const MANIFEST_KEYS: [&str; 12] = [
    "build_id",
    "format_version",
    "match_id",
    "seed",
    "teams",
    "roster",
    "pitch",
    "ticks_per_second",
    "frame_every_ticks",
    "attack_direction",
    "result",
    "segments",
];

/// The pinned SHA-256 of the published manifest body of the hand-built
/// match.
///
/// A golden value: the manifest body is the wire format of a match, so
/// a change in its bytes — a renamed field, a moved key, a changed
/// state — changes the hash and fails this test.
const GOLDEN_MANIFEST_BODY_HASH: &str =
    "62eaa125780fae7ed9df882f78948710a9af3a33e071308caca2d294e18ac122";

/// The keys of a JSON object, in the order they appear.
#[derive(Debug)]
struct KeyOrder(Vec<String>);

impl<'de> Deserialize<'de> for KeyOrder {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyVisitor;

        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = KeyOrder;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<KeyOrder, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    keys.push(key);
                    map.next_value::<IgnoredAny>()?;
                }
                Ok(KeyOrder(keys))
            }
        }

        deserializer.deserialize_map(KeyVisitor)
    }
}

/// The keys of a JSON object, in the order they appear.
fn key_order(bytes: &[u8]) -> Vec<String> {
    serde_json::from_slice::<KeyOrder>(bytes)
        .expect("invariant: a published body is a JSON object")
        .0
}

/// The published manifest body is the body the manifest route serves.
#[test]
fn the_published_manifest_body_is_the_body_the_manifest_route_serves() {
    let rendered = render_match(&fixture(), &hand_built_match());

    let route_body = serde_json::to_vec(&ManifestBody::ready(&rendered.manifest))
        .expect("invariant: the manifest body is JSON-serialisable");

    assert_eq!(
        &rendered.published.manifest_body()[..],
        route_body.as_slice(),
        "the published bytes are the bytes the route serves"
    );
}

/// The published manifest body carries the manifest and the ready
/// state, with the state last.
#[test]
fn the_published_manifest_body_carries_the_manifest_and_the_ready_state() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let body = rendered.published.manifest_body();

    let value: serde_json::Value =
        serde_json::from_slice(&body[..]).expect("invariant: a published body is JSON");
    assert_eq!(value["state"], "ready");
    assert_eq!(value["build_id"], rendered.manifest.build_id);
    assert_eq!(value["match_id"], rendered.manifest.match_id);
    assert_eq!(
        value["segments"],
        serde_json::to_value(&rendered.manifest.segments)
            .expect("invariant: the manifest's segments are JSON-serialisable")
    );

    let mut expected: Vec<String> = MANIFEST_KEYS.iter().map(|key| (*key).to_string()).collect();
    expected.push("state".to_string());
    assert_eq!(
        key_order(&body),
        expected,
        "the manifest's fields come first, in order, and the state last"
    );
}

/// The published manifest body is pinned: the wire format of a match
/// does not change unnoticed.
#[test]
fn the_published_manifest_body_is_pinned() {
    let rendered = render_match(&fixture(), &hand_built_match());

    assert_eq!(
        hex(&sha256(&rendered.published.manifest_body())),
        GOLDEN_MANIFEST_BODY_HASH,
        "the published manifest body must not change"
    );
}

/// Every published segment body is the body the segment route serves,
/// and it hashes to the hash the manifest publishes.
#[test]
fn every_published_segment_body_is_the_body_the_segment_route_serves() {
    let rendered = render_match(&fixture(), &hand_built_match());

    for (row, stored) in rendered.manifest.segments.iter().zip(&rendered.segments) {
        let published = rendered
            .published
            .segment(row.segment)
            .expect("invariant: the manifest lists the segment");

        let served = stored
            .decompress()
            .expect("invariant: a stored segment decompresses");
        assert_eq!(
            &published.body[..],
            served.as_slice(),
            "segment {}: the published body is the body the route serves",
            row.segment
        );
        assert_eq!(
            &published.gzipped_body[..],
            stored.gzipped_bytes(),
            "segment {}: the published gzip coding is the stored bytes",
            row.segment
        );
        assert_eq!(
            hex(&sha256(&published.body)),
            row.sha256,
            "segment {}: the published body hashes to the manifest's hash",
            row.segment
        );
        assert_eq!(
            u64::try_from(published.body.len()).unwrap_or(u64::MAX),
            row.uncompressed_bytes,
            "segment {}: the published body is as long as the manifest says",
            row.segment
        );
    }
}

/// The index covers exactly the manifest's segments.
#[test]
fn the_index_covers_exactly_the_manifests_segments() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let numbers: Vec<u32> = rendered
        .manifest
        .segments
        .iter()
        .map(|row| row.segment)
        .collect();

    assert_eq!(
        rendered.published.len(),
        numbers.len(),
        "every segment of the manifest is published once"
    );
    assert!(!rendered.published.is_empty());

    for row in &rendered.manifest.segments {
        let published = rendered
            .published
            .segment(row.segment)
            .expect("invariant: the manifest lists the segment");
        assert_eq!(
            hex(&sha256(&published.body)),
            row.sha256,
            "segment {}: the published body is the segment the manifest lists",
            row.segment
        );
    }

    // The manifest numbers its segments from zero, so a number it does
    // not list names nothing.
    let beyond = u32::try_from(numbers.len()).unwrap_or(u32::MAX);
    assert!(
        rendered.published.segment(beyond).is_none(),
        "a number beyond the last segment names nothing"
    );
    assert!(
        rendered.published.segment(u32::MAX).is_none(),
        "a number no match reaches names nothing"
    );
}

/// Publishing happens once per render, not once per read.
#[test]
fn publishing_the_same_match_twice_produces_the_same_bytes() {
    let fixture = fixture();
    let completed = hand_built_match();

    let first = render_match(&fixture, &completed);
    let second = render_match(&fixture, &completed);

    assert_eq!(
        first.published.manifest_body(),
        second.published.manifest_body(),
        "the published manifest body is the same bytes"
    );
    for row in &first.manifest.segments {
        let left = first
            .published
            .segment(row.segment)
            .expect("invariant: the manifest lists the segment");
        let right = second
            .published
            .segment(row.segment)
            .expect("invariant: the manifest lists the segment");
        assert_eq!(left.body, right.body, "the published body is the same bytes");
        assert_eq!(
            left.gzipped_body, right.gzipped_body,
            "the published gzip coding is the same bytes"
        );
    }
}

/// The published bytes are shared, never copied.
#[test]
fn the_published_bytes_are_shared_and_never_copied() {
    let rendered = render_match(&fixture(), &hand_built_match());
    let clone = rendered.published.clone();

    assert_eq!(
        clone.manifest_body().as_ptr(),
        rendered.published.manifest_body().as_ptr(),
        "a clone shares the manifest's bytes instead of copying them"
    );

    let published = rendered
        .published
        .segment(0)
        .expect("invariant: the manifest lists the segment");
    let cloned = clone
        .segment(0)
        .expect("invariant: the manifest lists the segment");
    assert_eq!(
        cloned.body.as_ptr(),
        published.body.as_ptr(),
        "a clone shares the segment's body instead of copying it"
    );
    assert_eq!(
        published.gzipped_body.as_ptr(),
        rendered.segments[0].gzipped_body.as_ptr(),
        "the gzip coding is shared with the stored segment instead of being copied"
    );
}
