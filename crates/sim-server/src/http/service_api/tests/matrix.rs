//! The status matrix, one table of rows (server-API ticket 08).
//!
//! Every row of the matrix is a case in the table: a request goes
//! through the router with `tower::ServiceExt::oneshot`, so no socket
//! is opened and no handler is called directly. The service under test
//! is driven by the fake runners, never by the real simulation.
//!
//! The rows of the status matrix live in `matrix_table`; this module
//! holds the assertions every row is checked against.

use super::matrix_table::matrix_rows;
use super::*;

/// Every row of the matrix is served as the matrix says.
#[tokio::test]
async fn the_status_matrix_is_served() {
    let rows = matrix_rows().await;

    for row in &rows {
        let router = router(Arc::clone(&row.service), http_config(None, &[]));
        let response = router
            .oneshot(row_request(row))
            .await
            .expect("invariant: the router answers every request");
        let (status, headers, body) = decoded_parts(response).await;

        let json = assert_answer(row, status, &headers, &body);
        assert_representation(row, &headers, &json);
        if let (Some(runs), Some(expected)) = (&row.runs, row.expected_runs) {
            assert_eq!(
                runs(),
                expected,
                "{}: the match ran the wrong number of times",
                row.name
            );
        }

        row.release();
    }
}

/// The request a row sends.
fn row_request(row: &Row) -> Request<Body> {
    let mut request = request(row.method.clone(), row.path);
    if row.representation == Representation::Gzip {
        request.headers_mut().insert(
            header::ACCEPT_ENCODING,
            "gzip".parse().expect("invariant: a valid header value"),
        );
    }
    request
}

/// The assertions every row shares: the status, the state and the
/// headers that do not depend on the representation.
fn assert_answer(
    row: &Row,
    status: StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> serde_json::Value {
    assert_eq!(status, row.status, "{}: wrong status; body: {body}", row.name);
    let json: serde_json::Value =
        serde_json::from_str(body).expect("invariant: a response body is JSON");    assert_eq!(
        json["state"], row.state,
        "{}: wrong state; body: {body}",
        row.name
    );

    let cache = headers
        .get(header::CACHE_CONTROL)
        .map(|value| value.to_str().unwrap_or_default().to_string());
    assert_eq!(
        cache.as_deref(),
        Some(row.cache),
        "{}: wrong cache header",
        row.name
    );
    assert_eq!(
        headers.contains_key(header::RETRY_AFTER),
        row.retry_after,
        "{}: wrong Retry-After header",
        row.name
    );
    assert_eq!(
        json.get("retry_after_ms").is_some(),
        row.retry_after_ms,
        "{}: wrong retry_after_ms in the body; body: {body}",
        row.name
    );

    if let Some(code) = row.code {
        assert_eq!(json["code"], code, "{}: wrong failure code", row.name);
        assert!(
            json["message"].is_string(),
            "{}: the failure must keep its message; body: {body}",
            row.name
        );
        assert!(
            json["last_good_tick"].is_number(),
            "{}: the failure must keep its last_good_tick; body: {body}",
            row.name
        );
    }
    json
}

/// The assertions of the representation: the coding the row asked for,
/// the validator that describes it and the `Vary` that keeps the two
/// representations apart.
fn assert_representation(row: &Row, headers: &HeaderMap, json: &serde_json::Value) {
    let gzipped = row.representation == Representation::Gzip;
    assert_eq!(
        headers.contains_key(header::CONTENT_ENCODING),
        gzipped,
        "{}: wrong content coding",
        row.name
    );

    if row.representation == Representation::None {
        assert!(
            !headers.contains_key(header::ETAG),
            "{}: an answer without a manifest carries no validator",
            row.name
        );
        assert!(
            !varies_on_accept_encoding(headers),
            "{}: an answer without a manifest names no representation",
            row.name
        );
        return;
    }

    let build_id = json["build_id"].as_str().unwrap_or_default().to_string();
    assert!(!build_id.is_empty(), "{}: the manifest has no build id", row.name);
    let etag = headers
        .get(header::ETAG)
        .map(|value| value.to_str().unwrap_or_default().to_string())
        .unwrap_or_default();
    let expected = if gzipped {
        gzip_validator(&build_id)
    } else {
        identity_validator(&build_id)
    };
    assert_eq!(
        etag, expected,
        "{}: the ETag must be the validator of the representation",
        row.name
    );
    assert!(
        varies_on_accept_encoding(headers),
        "{}: the manifest must vary on the accepted coding",
        row.name
    );
    assert!(
        json["result"]["score"].is_array(),
        "{}: the manifest must carry its result",
        row.name
    );
    assert!(
        json["segments"].is_array(),
        "{}: the manifest must carry its segments",
        row.name
    );
}
