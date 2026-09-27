//! Small HTTP building blocks that would otherwise be extra dependencies:
//! CORS, static file serving, rate limiting, and a response-decorating
//! middleware that adds security headers plus request logging.

pub mod cors;
pub mod ratelimit;
pub mod static_files;

use actix_web::body::MessageBody;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::HttpRequest;
use actix_web::Error;
use std::future::{ready, Ready};
use std::rc::Rc;
use std::time::Instant;

/// Adds the baseline security headers to every response and logs one line per
/// request with a correlation id.
///
/// Written as an explicit `Transform` rather than a bare `async fn` taking
/// `Next<B>` so the body type is threaded through concretely: `Compress` and
/// `Cors` both change it, and a generic middleware in that chain fails to
/// unify.
pub struct Decorate;


impl<S, B> Transform<S, ServiceRequest> for Decorate
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = DecorateService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(DecorateService {
            service: Rc::new(service),
        }))
    }
}

pub struct DecorateService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for DecorateService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let id = request_id();
        let method = req.method().to_string();
        let path = req.path().to_string();
        let start = Instant::now();
        let is_preflight = req.method() == actix_web::http::Method::OPTIONS;

        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;

            {
                let h = res.headers_mut();
                h.insert(
                    HeaderName::from_static("x-content-type-options"),
                    HeaderValue::from_static("nosniff"),
                );
                h.insert(
                    HeaderName::from_static("referrer-policy"),
                    HeaderValue::from_static("strict-origin-when-cross-origin"),
                );
                h.insert(
                    HeaderName::from_static("x-frame-options"),
                    HeaderValue::from_static("DENY"),
                );
                h.insert(
                    HeaderName::from_static("permissions-policy"),
                    HeaderValue::from_static("geolocation=(), microphone=(), camera=(), interest-cohort=()"),
                );
                h.insert(
                    HeaderName::from_static("x-request-id"),
                    HeaderValue::from_str(&id).unwrap_or(HeaderValue::from_static("-")),
                );
                // The CSP is strict on purpose: the frontend is plain
                // HTML/CSS/JS with no inline event handlers, images come from a
                // small allow-list plus our own proxy, and no frame is allowed
                // in. `style-src` keeps `unsafe-inline` because the stylesheet
                // is inline in the HTML shell and the alternative is a nonce
                // threaded through every render.
                if !is_preflight {
                    h.insert(
                        HeaderName::from_static("content-security-policy"),
                        HeaderValue::from_static(
                            "default-src 'self'; \
                             script-src 'self'; \
                             style-src 'self' 'unsafe-inline'; \
                             img-src 'self' data: https:; \
                             font-src 'self' data:; \
                             connect-src 'self' https:; \
                             base-uri 'self'; \
                             form-action 'self'; \
                             frame-ancestors 'none'",
                        ),
                    );
                }
            }

            // The progress endpoint is polled on a timer by the UI; logging it
            // would bury everything else.
            if !path.starts_with("/api/sync/status") {
                println!(
                    "{} {} {} -> {} ({} мс)",
                    id,
                    method,
                    path,
                    res.status().as_u16(),
                    start.elapsed().as_millis()
                );
            }
            Ok(res)
        })
    }
}

/// A short id from the OS CSPRNG, for correlating log lines only.
fn request_id() -> String {
    let mut buf = [0u8; 6];
    if getrandom::getrandom(&mut buf).is_err() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        buf = n.to_le_bytes()[..6].try_into().unwrap_or([0; 6]);
    }
    hex::encode(buf)
}


/// Same as [`client_key`] but for handlers, which receive an `HttpRequest`.
pub fn client_key_from(req: &HttpRequest) -> String {
    if let Some(v) = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = v.split(',').next() {
            let t = first.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    req.connection_info()
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
