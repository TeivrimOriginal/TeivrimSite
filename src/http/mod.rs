//! Small HTTP building blocks that would otherwise be extra dependencies:
//! CORS, static file serving, rate limiting, and a response-decorating
//! middleware that adds security headers plus request logging.

pub mod cors;
pub mod ratelimit;
pub mod static_files;

use actix_web::body::MessageBody;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::Error;
use actix_web::HttpRequest;
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
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

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
                    HeaderValue::from_static(
                        "geolocation=(), microphone=(), camera=(), interest-cohort=()",
                    ),
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
///
/// The fallback is there so a request still gets an id if the CSPRNG is
/// unavailable, which is why the buffer is sized to what the fallback actually
/// produces: `subsec_nanos` is a `u32`, so its byte array is four bytes long and
/// slicing six out of it would panic on the request path.
fn request_id() -> String {
    let mut buf = [0u8; 4];
    if getrandom::getrandom(&mut buf).is_err() {
        buf = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            .to_le_bytes();
    }
    hex::encode(buf)
}

/// Same as [`client_key`] but for handlers, which receive an `HttpRequest`.
///
/// The first hop of `x-forwarded-for` wins, because that is the client and
/// every proxy after it appended its own address. This trusts the header: a
/// deployment behind a proxy that overwrites it is fine, and one that only
/// appends to it lets a caller pick its own rate-limit key by sending the
/// header directly. Both are a deployment decision, so what is pinned here is
/// the documented behaviour rather than a claim that it is safe everywhere.
pub fn client_key_from(req: &HttpRequest) -> String {
    if let Some(v) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::dev::ServiceResponse;
    use actix_web::http::StatusCode;
    use actix_web::test::{self, TestRequest};
    use actix_web::{web, HttpResponse};

    fn req() -> actix_web::HttpRequest {
        TestRequest::default().to_http_request()
    }

    // ------------------------------------------------------- request ids

    #[test]
    fn a_request_id_is_eight_hex_characters() {
        // The header is sized to fit whatever the fallback produces, so a
        // length change here is a bug in the fallback, not in this test.
        for _ in 0..20 {
            let id = request_id();
            assert_eq!(id.len(), 8, "id: {}", id);
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "id: {}", id);
        }
    }

    #[test]
    fn request_ids_do_not_all_look_the_same() {
        // A constant id would make the log impossible to correlate, and the
        // only way to notice is to ask twice.
        let ids: std::collections::HashSet<String> = (0..50).map(|_| request_id()).collect();
        assert!(ids.len() > 45, "слишком мало разных id: {}", ids.len());
    }

    // ------------------------------------------------------ the client key

    #[test]
    fn without_a_forwarded_header_the_peer_address_is_the_key() {
        // `TestRequest` reports no peer, so this also covers the last resort.
        assert_eq!(client_key_from(&req()), "unknown");
    }

    #[test]
    fn the_first_hop_of_the_forwarded_header_is_the_client() {
        // Every proxy after the first appends its own address, so taking the
        // whole header would key the limiter on the nearest proxy and give
        // every visitor behind it one shared budget.
        let r = TestRequest::default()
            .insert_header(("x-forwarded-for", "203.0.113.7, 10.0.0.1, 10.0.0.2"))
            .to_http_request();
        assert_eq!(client_key_from(&r), "203.0.113.7");
    }

    #[test]
    fn a_forwarded_header_with_surrounding_space_is_trimmed() {
        let r = TestRequest::default()
            .insert_header(("x-forwarded-for", "  203.0.113.7 , 10.0.0.1"))
            .to_http_request();
        assert_eq!(client_key_from(&r), "203.0.113.7");
    }

    #[test]
    fn an_empty_forwarded_header_falls_through_instead_of_keying_on_nothing() {
        // An empty key would put every such caller in one bucket, which is the
        // opposite of what a limiter is for.
        for value in ["", "   ", ", 10.0.0.1", " , "] {
            let r = TestRequest::default()
                .insert_header(("x-forwarded-for", value))
                .to_http_request();
            assert_eq!(client_key_from(&r), "unknown", "значение {:?}", value);
        }
    }

    // ------------------------------------------------------ the middleware

    async fn decorated(method: &str) -> ServiceResponse {
        use actix_web::App;

        let app = test::init_service(
            App::new()
                .wrap(Decorate)
                .route(
                    "/x",
                    web::get().to(|| async { HttpResponse::Ok().finish() }),
                )
                .route(
                    "/x",
                    web::post().to(|| async { HttpResponse::Ok().finish() }),
                ),
        )
        .await;
        let req = match method {
            "OPTIONS" => TestRequest::default()
                .method(actix_web::http::Method::OPTIONS)
                .uri("/x")
                .to_request(),
            "POST" => TestRequest::post().uri("/x").to_request(),
            _ => TestRequest::get().uri("/x").to_request(),
        };
        test::call_service(&app, req).await
    }

    #[tokio::test]
    async fn every_response_carries_the_security_headers() {
        let res = decorated("GET").await;
        assert_eq!(res.status(), StatusCode::OK);
        let h = res.headers();
        for name in [
            "x-content-type-options",
            "referrer-policy",
            "x-frame-options",
            "permissions-policy",
            "x-request-id",
            "content-security-policy",
        ] {
            assert!(h.contains_key(name), "нет заголовка {}", name);
        }
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(h.get("x-frame-options").unwrap(), "DENY");
        let csp = h.get("content-security-policy").unwrap().to_str().unwrap();
        assert!(csp.contains("default-src 'self'"), "csp: {}", csp);
        assert!(csp.contains("frame-ancestors 'none'"), "csp: {}", csp);
    }

    #[tokio::test]
    async fn the_request_id_header_is_the_correlation_id() {
        // It has to look like an id, not like a placeholder, or the log line
        // and the response cannot be tied together.
        let res = decorated("POST").await;
        let id = res
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(id.len(), 8, "id: {}", id);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "id: {}", id);
    }

    #[tokio::test]
    async fn a_preflight_gets_no_content_security_policy() {
        // A CSP on a 204 preflight confuses some browsers into caching the
        // empty answer instead of the real one.
        let res = decorated("OPTIONS").await;
        assert!(
            !res.headers().contains_key("content-security-policy"),
            "CSP на preflight"
        );
        assert_eq!(
            res.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }
}
