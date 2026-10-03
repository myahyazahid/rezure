//! Minimal local reverse proxy inserted between cloudflared and nginx, whose
//! only job is rewriting the one thing a raw byte tunnel can't fix on its
//! own: a `Location:` redirect that names the project's own `.test` domain.
//!
//! # Why this exists
//!
//! nginx routes every project off the `Host` header (`services::vhosts`), so
//! whatever reaches it has to carry the project's real domain
//! (`fmt-app.test`) as `Host` — there's no other way for it to pick the
//! right vhost. But most frameworks build absolute redirect URLs from that
//! same Host header (Laravel's `url()`/`redirect()` among them). A
//! `302 Location: http://fmt-app.test/login` reaches an external browser
//! completely unchanged through a raw tunnel, and `fmt-app.test` resolves to
//! nothing outside this machine — the browser's *next* request goes
//! straight to a dead domain instead of back through the tunnel. That is
//! what a phone or a friend's laptop hitting a shared project actually saw:
//! `DNS_PROBE_FINISHED_NXDOMAIN` on `fmt-app.test`, not on the tunnel URL.
//!
//! This proxy sits between cloudflared and nginx specifically to catch that
//! one case: it forwards every request to nginx with the domain Host header
//! nginx needs, and on the way back rewrites only a `Location` header whose
//! host is exactly the project's domain to point at the tunnel's own public
//! hostname instead. Nothing else about the response is touched — an
//! absolute link written directly into an HTML body (`<img
//! src="http://fmt-app.test/logo.png">` rather than a relative path) is not
//! covered, only redirect headers are.
//!
//! # Why a real HTTP server crate instead of hand-rolled parsing
//!
//! `hyper`, `hyper-util` and `http-body-util` are already compiled into this
//! binary as transitive dependencies of `reqwest` — promoting them to direct
//! dependencies for a correct HTTP/1.1 server is far safer than hand-parsing
//! chunked encoding and keep-alive semantics by hand for what is, on the
//! wire, a real HTTP proxy.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

use bytes::Bytes;
use http::header::{
    ACCEPT_ENCODING, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, LOCATION, TRANSFER_ENCODING,
};
use http::HeaderMap;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::utils::error::AppError;

/// Never follows redirects itself — the whole point is relaying a 3xx to the
/// browser (with its `Location` rewritten below), not silently resolving it
/// server-side and handing back the final page instead.
fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default()
    })
}

/// A running rewrite proxy, bound to an ephemeral local port. Dropping it
/// stops accepting new connections; a request already in flight finishes on
/// its own, since each is its own spawned task.
#[derive(Debug)]
pub struct Proxy {
    pub port: u16,
    /// Set once cloudflared has reported the tunnel's public hostname.
    /// `None` until then, so a request that lands before it's known (there
    /// shouldn't be one — nobody has the URL yet) is forwarded with no
    /// rewrite rather than blocked on it.
    public_host: Arc<Mutex<Option<String>>>,
    /// Held only to be dropped: dropping the sender resolves the accept
    /// loop's `shutdown_rx` (as an error, which is still a completion),
    /// which is its cue to stop. No explicit `.send()` needed.
    _shutdown: oneshot::Sender<()>,
}

impl Proxy {
    pub fn set_public_host(&self, host: String) {
        *self.public_host.lock().unwrap_or_else(|e| e.into_inner()) = Some(host);
    }
}

/// Starts the proxy. Every request it receives is forwarded to
/// `127.0.0.1:80` (nginx) with `Host` forced to `project_domain` — the value
/// nginx's `server_name` for this project expects (see `services::vhosts`).
pub async fn spawn(project_domain: String) -> Result<Proxy, AppError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| AppError::ShareFailed(format!("couldn't open a local proxy port: {e}")))?;
    let port = listener
        .local_addr()
        .map(|addr: SocketAddr| addr.port())
        .map_err(|e| AppError::ShareFailed(format!("couldn't read the proxy's port: {e}")))?;

    let public_host: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    let accept_domain = project_domain;
    let accept_public_host = public_host.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let domain = accept_domain.clone();
                    let public_host = accept_public_host.clone();
                    tokio::spawn(async move {
                        let io = TokioIo::new(stream);
                        let service = service_fn(move |req| {
                            handle(req, domain.clone(), public_host.clone())
                        });
                        // Best-effort: a client that disconnects mid-request
                        // (a closed browser tab) is not this proxy's problem.
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(io, service)
                            .await;
                    });
                }
            }
        }
    });

    Ok(Proxy {
        port,
        public_host,
        _shutdown: shutdown_tx,
    })
}

async fn handle(
    req: Request<Incoming>,
    project_domain: String,
    public_host: Arc<Mutex<Option<String>>>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    match forward(req, &project_domain, &public_host).await {
        Ok(response) => Ok(response),
        Err(err) => Ok(Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Full::new(Bytes::from(format!("share proxy error: {err}"))))
            .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))),
    }
}

async fn forward(
    req: Request<Incoming>,
    project_domain: &str,
    public_host: &Arc<Mutex<Option<String>>>,
) -> Result<Response<Full<Bytes>>, AppError> {
    let (parts, body) = req.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| AppError::ShareFailed(format!("couldn't read the request body: {e}")))?
        .to_bytes();

    let target = format!(
        "http://127.0.0.1:80{}",
        parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/")
    );

    let method = Method::from_bytes(parts.method.as_str().as_bytes()).unwrap_or(Method::GET);
    let mut request = http_client().request(method, &target);
    for (name, value) in parts.headers.iter() {
        // `Host` is replaced outright below — it's the one header this
        // proxy exists to control. `Connection` is hop-by-hop and isn't
        // meant to be relayed to a second hop. `Accept-Encoding` is dropped
        // so nginx/PHP answer uncompressed — the response body gets
        // string-rewritten below (see `rewrite_domain_in_body`), which only
        // works on plain text, not a gzip/br frame.
        if *name == HOST || *name == CONNECTION || *name == ACCEPT_ENCODING {
            continue;
        }
        request = request.header(name, value);
    }
    request = request.header(HOST, project_domain);
    request = request.body(body_bytes);

    let response = request
        .send()
        .await
        .map_err(|e| AppError::ShareFailed(format!("couldn't reach nginx: {e}")))?;

    let status = response.status();
    let headers = response.headers().clone();
    let body_bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::ShareFailed(format!("couldn't read nginx's response: {e}")))?;

    let public_host = public_host
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    // A redirect header is one thing, but the same absolute-URL-from-Host
    // habit shows up baked directly into HTML/CSS/JS — Laravel's Vite plugin
    // in particular writes `<link href="http://fmt-app.test/build/...">`.
    // The request above dropped `Accept-Encoding` specifically so this is
    // always plain text when it's text at all, never a gzip/br frame this
    // can't safely search-and-replace inside.
    let body_bytes = match public_host.as_deref() {
        Some(public_host) if is_rewritable_text(&headers) => {
            match String::from_utf8(body_bytes.to_vec()) {
                Ok(text) => Bytes::from(rewrite_domain_in_body(&text, project_domain, public_host)),
                // Not actually UTF-8 text despite the content type — left
                // untouched rather than risk corrupting it.
                Err(_) => body_bytes,
            }
        }
        _ => body_bytes,
    };

    let mut builder = Response::builder().status(status);
    for (name, value) in headers.iter() {
        // Recomputed by hyper for the buffered `Full` body below — relaying
        // the originals (possibly `chunked`) could disagree with what's
        // actually being sent now that the whole response is buffered.
        if *name == CONNECTION || *name == TRANSFER_ENCODING || *name == CONTENT_LENGTH {
            continue;
        }
        if *name == LOCATION {
            if let (Ok(value_str), Some(public_host)) = (value.to_str(), public_host.as_deref()) {
                let rewritten = rewrite_location(value_str, project_domain, public_host);
                if let Ok(header_value) = http::HeaderValue::from_str(&rewritten) {
                    builder = builder.header(LOCATION, header_value);
                    continue;
                }
            }
        }
        builder = builder.header(name, value);
    }

    builder
        .body(Full::new(body_bytes))
        .map_err(|e| AppError::ShareFailed(format!("couldn't build the response: {e}")))
}

/// Rewrites `location` to point at `public_host` if — and only if — it's an
/// absolute URL whose host is exactly `project_domain`. A relative redirect
/// (`/login`) or one naming any other host (an OAuth provider, say) is
/// returned unchanged.
fn rewrite_location(location: &str, project_domain: &str, public_host: &str) -> String {
    let Ok(mut url) = url::Url::parse(location) else {
        return location.to_string();
    };
    let is_project_domain = url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(project_domain));
    if !is_project_domain {
        return location.to_string();
    }

    let _ = url.set_scheme("https");
    let _ = url.set_host(Some(public_host));
    let _ = url.set_port(None);
    url.to_string()
}

/// Whether a response's `Content-Type` is text worth searching for the
/// project's domain — HTML, CSS and JS are exactly where Laravel's Vite
/// plugin (and friends) write an absolute asset URL. Images, fonts and other
/// binary types are left alone: there's nothing textual in them to rewrite,
/// and treating them as UTF-8 would only risk corrupting real bytes.
fn is_rewritable_text(headers: &HeaderMap) -> bool {
    let Some(content_type) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    // Matched by prefix so a `; charset=UTF-8` suffix doesn't miss the type.
    const REWRITABLE_PREFIXES: &[&str] = &[
        "text/html",
        "text/css",
        "text/javascript",
        "application/javascript",
        "application/json",
        "text/plain",
        "image/svg+xml",
    ];
    REWRITABLE_PREFIXES
        .iter()
        .any(|prefix| content_type.starts_with(prefix))
}

/// Replaces every occurrence of `project_domain` in `body` with
/// `public_host` — scheme-prefixed first (`http://fmt-app.test` and
/// `https://fmt-app.test`, both forced to `https://<public_host>`, since the
/// tunnel is HTTPS-only), then whatever's left bare (a domain named with no
/// scheme, e.g. inside an inlined JSON config). Doing the prefixed forms
/// first means the bare pass only ever touches what they didn't.
fn rewrite_domain_in_body(body: &str, project_domain: &str, public_host: &str) -> String {
    body.replace(
        &format!("http://{project_domain}"),
        &format!("https://{public_host}"),
    )
    .replace(
        &format!("https://{project_domain}"),
        &format!("https://{public_host}"),
    )
    .replace(project_domain, public_host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_redirect_to_the_project_domain_is_rewritten() {
        let rewritten = rewrite_location(
            "http://fmt-app.test/login",
            "fmt-app.test",
            "basics-widely-jesus-resident.trycloudflare.com",
        );
        assert_eq!(
            rewritten,
            "https://basics-widely-jesus-resident.trycloudflare.com/login"
        );
    }

    #[test]
    fn a_relative_redirect_is_left_alone() {
        assert_eq!(
            rewrite_location("/login", "fmt-app.test", "tunnel.trycloudflare.com"),
            "/login"
        );
    }

    #[test]
    fn a_redirect_to_a_different_host_is_left_alone() {
        let location = "https://accounts.google.com/o/oauth2/auth";
        assert_eq!(
            rewrite_location(location, "fmt-app.test", "tunnel.trycloudflare.com"),
            location
        );
    }

    #[test]
    fn the_domain_match_is_case_insensitive() {
        let rewritten = rewrite_location(
            "http://FMT-APP.TEST/dashboard",
            "fmt-app.test",
            "tunnel.trycloudflare.com",
        );
        assert_eq!(rewritten, "https://tunnel.trycloudflare.com/dashboard");
    }

    #[test]
    fn query_and_fragment_survive_the_rewrite() {
        let rewritten = rewrite_location(
            "http://fmt-app.test/search?q=hi#top",
            "fmt-app.test",
            "tunnel.trycloudflare.com",
        );
        assert_eq!(
            rewritten,
            "https://tunnel.trycloudflare.com/search?q=hi#top"
        );
    }

    #[test]
    fn an_https_redirect_to_the_project_domain_is_also_rewritten() {
        // Unusual (the project is only ever served over plain http), but the
        // match is on host, not scheme, so this must not slip through.
        let rewritten = rewrite_location(
            "https://fmt-app.test/settings",
            "fmt-app.test",
            "tunnel.trycloudflare.com",
        );
        assert_eq!(rewritten, "https://tunnel.trycloudflare.com/settings");
    }

    #[test]
    fn a_vite_style_absolute_asset_link_is_rewritten_in_the_body() {
        let html =
            r#"<link href="http://rafkogap.test/build/assets/app-DpiTJpwR.css" rel="stylesheet">"#;
        let rewritten = rewrite_domain_in_body(
            html,
            "rafkogap.test",
            "eds-proprietary-saving-expert.trycloudflare.com",
        );
        assert_eq!(
            rewritten,
            r#"<link href="https://eds-proprietary-saving-expert.trycloudflare.com/build/assets/app-DpiTJpwR.css" rel="stylesheet">"#
        );
    }

    #[test]
    fn a_bare_domain_with_no_scheme_is_also_rewritten() {
        // e.g. a Vite/Laravel config object serialized straight into the page.
        let json = r#"{"host":"rafkogap.test","https":false}"#;
        let rewritten = rewrite_domain_in_body(json, "rafkogap.test", "tunnel.trycloudflare.com");
        assert_eq!(
            rewritten,
            r#"{"host":"tunnel.trycloudflare.com","https":false}"#
        );
    }

    #[test]
    fn a_body_with_no_mention_of_the_domain_is_untouched() {
        let html = "<h1>Distributor Mur Baut</h1>";
        assert_eq!(
            rewrite_domain_in_body(html, "rafkogap.test", "tunnel.trycloudflare.com"),
            html
        );
    }

    #[test]
    fn html_and_css_and_js_content_types_are_rewritable() {
        for content_type in [
            "text/html; charset=UTF-8",
            "text/css",
            "application/javascript; charset=utf-8",
            "image/svg+xml",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, content_type.parse().unwrap());
            assert!(
                is_rewritable_text(&headers),
                "{content_type} should be rewritable"
            );
        }
    }

    #[test]
    fn binary_content_types_are_not_rewritable() {
        for content_type in ["image/png", "font/woff2", "application/octet-stream"] {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, content_type.parse().unwrap());
            assert!(
                !is_rewritable_text(&headers),
                "{content_type} must not be treated as rewritable text"
            );
        }
    }

    #[test]
    fn a_response_with_no_content_type_is_not_rewritable() {
        assert!(!is_rewritable_text(&HeaderMap::new()));
    }
}
