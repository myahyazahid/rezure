//! A GET that never reads more than it was promised.
//!
//! For files whose size the caller already knows from an earlier, validated
//! response — a sticker, a QRIS image. The size is a promise made by the
//! server, so it is also the limit: a body that runs past it is a different
//! file than the one described, and reading on to find out how big it really
//! is would let a misbehaving server fill memory.

use std::time::Duration;

/// Why a download didn't produce bytes. Each caller words these for its own
/// page; none of them carries a transport error, which would leak URLs and
/// paths into a message people read.
#[derive(Debug, PartialEq, Eq)]
pub enum GetError {
    /// The HTTP client couldn't be built.
    Setup(String),
    /// No answer: offline, DNS, refused, timed out.
    Unreachable,
    /// The server answered something other than success.
    Status(reqwest::StatusCode),
    /// More bytes than the limit — declared up front or counted on arrival.
    TooBig,
    /// The connection dropped mid-body.
    CutOff,
}

/// Fetches `url`, reading at most `limit` bytes.
///
/// A declared `Content-Length` over the limit is refused before the body is
/// read; without one (a chunked reply), bytes are counted as they arrive and
/// the download stops the moment the limit is passed.
pub async fn get_bounded(url: &str, limit: u64, timeout: Duration) -> Result<Vec<u8>, GetError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| GetError::Setup(e.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| GetError::Unreachable)?;
    if !response.status().is_success() {
        return Err(GetError::Status(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(GetError::TooBig);
    }

    let mut bytes = Vec::with_capacity(limit.min(1024 * 1024) as usize);
    while let Some(chunk) = response.chunk().await.map_err(|_| GetError::CutOff)? {
        if (bytes.len() + chunk.len()) as u64 > limit {
            return Err(GetError::TooBig);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_http::{serve, Reply};

    const WAIT: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn a_body_within_the_limit_is_returned_whole() {
        let server = serve(vec![("/f", Reply::bytes(b"0123456789"))]);
        let url = format!("{}/f", server.base_url);

        assert_eq!(get_bounded(&url, 10, WAIT).await.unwrap(), b"0123456789");
        assert_eq!(get_bounded(&url, 100, WAIT).await.unwrap(), b"0123456789");
    }

    #[tokio::test]
    async fn a_declared_length_over_the_limit_is_refused() {
        let server = serve(vec![("/f", Reply::bytes(b"0123456789"))]);
        let url = format!("{}/f", server.base_url);

        assert_eq!(get_bounded(&url, 9, WAIT).await, Err(GetError::TooBig));
    }

    #[tokio::test]
    async fn an_undeclared_length_is_counted_as_it_arrives() {
        let server = serve(vec![("/f", Reply::bytes(b"0123456789").without_length())]);
        let url = format!("{}/f", server.base_url);

        assert_eq!(get_bounded(&url, 9, WAIT).await, Err(GetError::TooBig));
        assert_eq!(get_bounded(&url, 10, WAIT).await.unwrap(), b"0123456789");
    }

    #[tokio::test]
    async fn a_failure_status_and_an_unreachable_host_are_told_apart() {
        let server = serve(vec![]);
        let url = format!("{}/missing", server.base_url);

        assert_eq!(
            get_bounded(&url, 10, WAIT).await,
            Err(GetError::Status(reqwest::StatusCode::NOT_FOUND))
        );
        assert_eq!(
            get_bounded("http://127.0.0.1:1/f", 10, WAIT).await,
            Err(GetError::Unreachable)
        );
    }
}
