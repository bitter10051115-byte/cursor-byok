//! Selects local handling or the configured official Cursor upstream.
use std::time::Instant;

use axum::{
    body::{to_bytes, Body, Bytes},
    extract::Extension,
    http::{header, HeaderMap, Request, Response},
};

use crate::Result;

const CURSOR_UPSTREAM: &str = "https://api2.cursor.sh";
pub const UPSTREAM_URL_HEADER: &str = "x-server-upstream-url";

pub(crate) fn cursor_upstream() -> &'static str {
    CURSOR_UPSTREAM
}

#[derive(Clone)]
pub struct CursorProxy {
    clients: crate::network::NetworkClients,
    upstream: String,
}

pub struct BufferedResponse {
    pub status: axum::http::StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: Bytes,
}

impl BufferedResponse {
    pub fn into_response(self) -> Response<Body> {
        let body = self.body.clone();
        self.with_body(body)
    }

    pub fn with_body(mut self, body: Bytes) -> Response<Body> {
        self.headers.insert(
            header::CONTENT_LENGTH,
            body.len()
                .to_string()
                .parse()
                .expect("body length is always a valid header value"),
        );
        let mut response = Response::new(Body::from(body));
        *response.status_mut() = self.status;
        *response.headers_mut() = self.headers;
        response
    }
}

impl CursorProxy {
    pub fn cursor(clients: crate::network::NetworkClients) -> Self {
        Self {
            clients,
            upstream: CURSOR_UPSTREAM.into(),
        }
    }

    async fn client(&self) -> Result<reqwest::Client> {
        self.clients.cursor_client().await
    }
}

pub async fn forward(
    Extension(proxy): Extension<CursorProxy>,
    request: Request<Body>,
) -> Result<Response<Body>> {
    forward_request(&proxy, request, None).await
}

/// Opens one upstream stream and reports once its request body has been sent.
///
/// The callback runs before response headers arrive, so the caller can submit
/// the matching write (`BidiAppend`) after the read stream exists.
pub(crate) async fn post_stream(
    proxy: &CursorProxy,
    url: String,
    mut headers: HeaderMap,
    body: Bytes,
    on_request_sent: impl FnOnce() + Send + 'static,
) -> Result<reqwest::Response> {
    headers.remove(UPSTREAM_URL_HEADER);
    headers.remove(header::HOST);
    headers.remove(header::CONTENT_LENGTH);
    remove_hop_by_hop_headers(&mut headers);
    headers.insert(
        header::ACCEPT_ENCODING,
        axum::http::HeaderValue::from_static("identity"),
    );
    headers.insert(
        "connect-accept-encoding",
        axum::http::HeaderValue::from_static("identity"),
    );
    proxy
        .client()
        .await?
        .post(url)
        .headers(headers)
        .body(body_notifying_when_sent(body, on_request_sent))
        .send()
        .await
        .map_err(crate::Error::from)
}

fn body_notifying_when_sent(
    body: Bytes,
    on_request_sent: impl FnOnce() + Send + 'static,
) -> reqwest::Body {
    struct NotifyOnDrop<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for NotifyOnDrop<F> {
        fn drop(&mut self) {
            if let Some(callback) = self.0.take() {
                callback();
            }
        }
    }
    let notify = NotifyOnDrop(Some(on_request_sent));
    reqwest::Body::wrap_stream(async_stream::stream! {
        let _notify = notify;
        yield Ok::<Bytes, std::io::Error>(body);
    })
}

pub(crate) async fn forward_to_service(
    proxy: &CursorProxy,
    request: Request<Body>,
    service_url: &str,
) -> Result<Response<Body>> {
    forward_request(proxy, request, Some(service_url)).await
}

async fn forward_request(
    proxy: &CursorProxy,
    request: Request<Body>,
    service_url: Option<&str>,
) -> Result<Response<Body>> {
    let started = Instant::now();
    let (parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map_or("/", |value| value.as_str())
        .to_owned();
    let url = match service_url {
        Some(service_url) => format!("{}{}", service_url.trim_end_matches('/'), path),
        None => upstream_url(&parts.headers, &proxy.upstream, &path)?,
    };

    let mut headers = parts.headers;
    headers.remove(UPSTREAM_URL_HEADER);
    headers.remove(header::HOST);
    remove_hop_by_hop_headers(&mut headers);

    let client = proxy.client().await?;
    let upstream = client
        .request(parts.method.clone(), url)
        .headers(headers)
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await;

    let upstream = match upstream {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(
                method = %parts.method,
                path,
                elapsed_ms = started.elapsed().as_millis(),
                %error,
                "Cursor upstream request failed"
            );
            return Err(error.into());
        }
    };

    let status = upstream.status();
    let mut response_headers = upstream.headers().clone();
    remove_hop_by_hop_headers(&mut response_headers);
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;

    tracing::info!(
        method = %parts.method,
        path,
        %status,
        elapsed_ms = started.elapsed().as_millis(),
        "forwarded Cursor backend request"
    );
    Ok(response)
}

pub async fn forward_buffered(
    proxy: &CursorProxy,
    request: Request<Body>,
) -> Result<BufferedResponse> {
    let (parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map_or("/", |value| value.as_str());
    let url = upstream_url(&parts.headers, &proxy.upstream, path)?;
    let mut headers = parts.headers;
    headers.remove(UPSTREAM_URL_HEADER);
    headers.remove(header::HOST);
    remove_hop_by_hop_headers(&mut headers);
    headers.insert(
        "connect-accept-encoding",
        axum::http::HeaderValue::from_static("identity"),
    );
    headers.insert(
        header::ACCEPT_ENCODING,
        axum::http::HeaderValue::from_static("identity"),
    );
    let body = to_bytes(body, usize::MAX)
        .await
        .map_err(|error| crate::Error::Protocol(format!("cannot read request body: {error}")))?;
    let upstream = proxy
        .client()
        .await?
        .request(parts.method, url)
        .headers(headers)
        .body(body)
        .send()
        .await?;
    let status = upstream.status();
    let mut headers = upstream.headers().clone();
    remove_hop_by_hop_headers(&mut headers);
    let body = upstream.bytes().await?;
    Ok(BufferedResponse {
        status,
        headers,
        body,
    })
}

fn upstream_url(headers: &axum::http::HeaderMap, fallback: &str, path: &str) -> Result<String> {
    let Some(value) = headers.get(UPSTREAM_URL_HEADER) else {
        return Ok(format!("{fallback}{path}"));
    };
    let value = value
        .to_str()
        .map_err(|error| crate::Error::Protocol(format!("invalid upstream URL header: {error}")))?;
    let url = reqwest::Url::parse(value)
        .map_err(|error| crate::Error::Protocol(format!("invalid upstream URL: {error}")))?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https" || !crate::local_app::proxy_host_allowed(host) {
        return Err(crate::Error::Protocol(
            "upstream URL must target a Cursor HTTPS host".into(),
        ));
    }
    Ok(url.into())
}

fn remove_hop_by_hop_headers(headers: &mut axum::http::HeaderMap) {
    let connection_headers = headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for name in connection_headers {
        headers.remove(name);
    }
    for name in [
        header::CONNECTION,
        header::PROXY_AUTHENTICATE,
        header::PROXY_AUTHORIZATION,
        header::TE,
        header::TRAILER,
        header::TRANSFER_ENCODING,
        header::UPGRADE,
    ] {
        headers.remove(name);
    }
    headers.remove("keep-alive");
}
