//! The round trip of the segment URL scheme (server-API ticket 09).
//!
//! The manifest publishes a URL for every segment, the router routes
//! it and the segment route serves the bytes behind it. This is where
//! the three are held together: every URL a ready match's manifest
//! publishes is asked for through the real router, and the body that
//! comes back must be the body the manifest hashed.
//!
//! Every case goes through the router with `tower::ServiceExt::oneshot`
//! and the fake runners, so no socket is opened and the real
//! simulation never runs.

use sha2::{Digest, Sha256};

use crate::api::render::SegmentUrl;

use super::*;

/// Every URL the manifest publishes is served, and the body it
/// answers with is the body the manifest hashed.
///
/// This is the round trip of the URL scheme: the manifest publishes
/// the URL, the router routes it, and the segment behind it is the
/// one the manifest's hash covers. Every segment of the match is
/// asked for, so an off-by-one in the numbering cannot hide.
#[tokio::test]
async fn every_published_segment_url_serves_the_body_the_manifest_hashed() {
    let service = ready_service("m-0001").await;
    let router = router(Arc::clone(&service), http_config(None, &[]));
    let MatchState::Ready { published } = service.status("m-0001") else {
        panic!("invariant: the match under test is ready");
    };
    let manifest: serde_json::Value =
        serde_json::from_slice(&published.manifest_body()[..])
            .expect("invariant: the published manifest body is JSON");
    let rows = manifest["segments"]
        .as_array()
        .expect("invariant: the manifest lists its segments");
    assert!(!rows.is_empty(), "the manifest must list its segments");

    // The manifest numbers its segments from zero, without a gap.
    let numbers: Vec<u64> = rows
        .iter()
        .map(|row| {
            row["segment"]
                .as_u64()
                .expect("invariant: the manifest publishes a segment number")
        })
        .collect();
    let expected: Vec<u64> = (0..numbers.len() as u64).collect();
    assert_eq!(numbers, expected, "the segments are numbered from zero");

    for row in rows {
        let url = row["url"]
            .as_str()
            .expect("invariant: the manifest publishes a segment URL");
        let sha256 = row["sha256"]
            .as_str()
            .expect("invariant: the manifest publishes a segment hash");
        let number = row["segment"]
            .as_u64()
            .expect("invariant: the manifest publishes a segment number");

        // The scheme's parser reads the published URL back into the
        // parts it was built from: the URL names the segment this row
        // lists, of this match, of this build.
        let parts = SegmentUrl::parse(url).expect("invariant: a published URL is a segment URL");
        assert_eq!(parts.match_id, "m-0001");
        assert_eq!(parts.build_id, published.build_id());
        assert_eq!(
            u64::from(parts.number),
            number,
            "the URL must name the segment the row lists"
        );

        let response = router
            .clone()
            .oneshot(request(Method::GET, url))
            .await
            .expect("invariant: the router answers every request");
        let (status, _headers, body) = response_bytes(response).await;

        assert_eq!(status, StatusCode::OK, "the router must serve {url}");
        assert_eq!(
            hex(&Sha256::digest(&body)),
            sha256,
            "the served body must hash to the hash the manifest publishes: {url}"
        );
    }
}
