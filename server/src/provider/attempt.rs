//! Sends Provider HTTP attempts, with a bounded retry for transient MaaS
//! rejections.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::{Error, Result};

use super::CallRecorder;

const MAX_MAAS_RETRY_ATTEMPTS: usize = 3;

#[derive(Debug)]
pub(crate) enum Attempt {
    Response(reqwest::Response),
    Cancelled,
}

pub(crate) async fn send_once<F>(
    label: &str,
    build: F,
    cancellation: &CancellationToken,
    recorder: Option<&CallRecorder>,
) -> Result<Attempt>
where
    F: FnOnce() -> reqwest::RequestBuilder,
{
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Ok(Attempt::Cancelled),
        response = build().send() => response,
    }?;
    if let Some(recorder) = recorder {
        recorder
            .response_headers(response.status().as_u16())
            .await?;
    }
    if response.status().is_success() {
        return Ok(Attempt::Response(response));
    }
    let status = response.status();
    let bytes = tokio::select! {
        _ = cancellation.cancelled() => return Ok(Attempt::Cancelled),
        bytes = response.bytes() => bytes,
    }?;
    Err(Error::Provider(format!(
        "{label} {status}: {}",
        String::from_utf8_lossy(&bytes)
    )))
}

/// Sends a request with a small number of retries for the upstream gateway's
/// intermittent `400001` rejection. The Cursor BYOK gateway returns this even
/// for byte-identical requests that succeed on other attempts, so the request
/// itself is not the problem and retrying avoids surfacing a spurious failure
/// to the client. Ordinary 4xx rejections still fail immediately.
pub(crate) async fn send_with_retry<F>(
    label: &str,
    mut build: F,
    cancellation: &CancellationToken,
    recorder: Option<&CallRecorder>,
) -> Result<Attempt>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match send_once(label, &mut build, cancellation, recorder).await {
            Ok(attempt) => return Ok(attempt),
            Err(error)
                if attempts < MAX_MAAS_RETRY_ATTEMPTS && is_transient_maas_rejection(&error) =>
            {
                let backoff = Duration::from_millis(200 * (1 << (attempts - 1)));
                tracing::warn!(
                    attempt = attempts,
                    %error,
                    backoff_ms = backoff.as_millis() as u64,
                    "retrying transient MaaS rejection"
                );
                tokio::select! {
                    _ = cancellation.cancelled() => return Ok(Attempt::Cancelled),
                    _ = tokio::time::sleep(backoff) => {}
                }
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_transient_maas_rejection(error: &Error) -> bool {
    let Error::Provider(message) = error else {
        return false;
    };
    let status = message
        .split_once(": ")
        .map_or(message.as_str(), |(head, _)| head)
        .split_whitespace()
        .find(|token| {
            token.len() == 3
                && token.starts_with('4')
                && token.bytes().all(|byte| byte.is_ascii_digit())
        })
        .unwrap_or_default();
    status == "400"
        && (message.contains("400001")
            || message.contains("rejected by an internal MaaS component"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn server(response: &'static [u8]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            socket.write_all(response).await.unwrap();
        });
        format!("http://{address}")
    }

    async fn server_responses(responses: &'static [(&'static str, &'static [u8])]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(body).await.unwrap();
            }
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn non_success_status_is_one_failed_attempt() {
        let url =
            server(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\n\r\ndown").await;
        let client = reqwest::Client::new();
        let error = send_once("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Provider(message) if message.contains("503") && message.contains("down"))
        );
    }

    #[tokio::test]
    async fn response_body_transport_failure_is_one_failed_attempt() {
        let url =
            server(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 100\r\n\r\nshort").await;
        let client = reqwest::Client::new();
        let error = send_once("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Http(_)),
            "expected a transport error, got {error:?}"
        );
    }

    #[tokio::test]
    async fn request_transport_failure_is_one_failed_attempt() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let url = format!("http://{address}");
        let client = reqwest::Client::new();
        let error = send_once("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Http(_)),
            "expected a transport error, got {error:?}"
        );
    }

    #[tokio::test]
    async fn transient_maas_rejection_is_retried_until_success() {
        const MAAS_BODY: &[u8] = br#"{"error":{"message":"The request is invalid: the request was rejected by an internal MaaS component. Please check the request body, required fields, and request format.","type":"invalid_request_error","param":"","code":"400001"}}"#;
        let url = server_responses(&[("400 Bad Request", MAAS_BODY), ("200 OK", b"ok")]).await;
        let client = reqwest::Client::new();
        let attempt = send_with_retry("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap();
        let Attempt::Response(response) = attempt else {
            panic!("expected a successful response after retry");
        };
        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn ordinary_rejected_request_is_not_retried() {
        let url = server_responses(&[(
            "400 Bad Request",
            b"{\"error\":{\"message\":\"unknown model\",\"type\":\"invalid_request_error\",\"code\":\"model_not_found\"}}",
        )])
        .await;
        let client = reqwest::Client::new();
        let error = send_with_retry("test", || client.get(&url), &CancellationToken::new(), None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Provider(message) if message.contains("400")));
    }
}
