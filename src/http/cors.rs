//! CORS without a dependency.
//!
//! The Android client and any future web client sit on a different origin from
//! the API, so the browser-conventional preflight has to be answered. Only
//! bearer tokens authenticate, never cookies, so there is no ambient authority
//! for a permissive policy to abuse; `CORS_ORIGINS` narrows the allow-list
//! when a deployment needs it.
//!
//! This middleware only *decorates* responses. Preflights are handled by an
//! explicit `OPTIONS` catch-all route inside the `/api` scope (see
//! `api::configure`), because a middleware cannot replace a response with a
//! different body type without pulling in `EitherBody` plumbing for no benefit.

use actix_web::body::MessageBody;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::http::header::{self, HeaderValue};
use actix_web::Error;
use std::future::{ready, Future, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};

#[derive(Clone)]
pub struct Cors {
    origins: Arc<Vec<String>>,
    allow_any: bool,
}

impl Cors {
    pub fn new(origins: Vec<String>) -> Cors {
        let allow_any = origins.iter().any(|o| o == "*");
        Cors {
            origins: Arc::new(origins),
            allow_any,
        }
    }

    fn allows(&self, origin: Option<&str>) -> bool {
        if self.allow_any {
            return true;
        }
        match origin {
            Some(o) => self.origins.iter().any(|a| a == o),
            None => false,
        }
    }

    /// Writes the CORS headers onto an arbitrary header map, so the preflight
    /// route can reuse the exact same policy.
    pub fn apply_headers(&self, h: &mut actix_web::http::header::HeaderMap, origin: Option<&str>) {
        if !self.allows(origin) {
            return;
        }
        if self.allow_any {
            h.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
        } else if let Some(o) = origin {
            if let Ok(v) = HeaderValue::from_str(o) {
                h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
                h.insert(header::VARY, HeaderValue::from_static("Origin"));
            }
        }
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PATCH, DELETE, OPTIONS"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static(
                "authorization, content-type, x-admin-token, x-requested-with",
            ),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("86400"),
        );
        h.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static("x-total-count, x-request-id"),
        );
    }

    pub fn request_origin(req: &ServiceRequest) -> Option<String> {
        req.headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }
}

impl<S, B> Transform<S, ServiceRequest> for Cors
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = CorsService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(CorsService {
            service: Rc::new(service),
            cors: self.clone(),
        }))
    }
}

pub struct CorsService<S> {
    service: Rc<S>,
    cors: Cors,
}

impl<S, B> Service<ServiceRequest> for CorsService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let origin = Cors::request_origin(&req);
        let cors = self.cors.clone();
        let fut = self.service.call(req);
        Box::pin(async move {
            let mut res = fut.await?;
            cors.apply_headers(res.headers_mut(), origin.as_deref());
            Ok(res)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::header;

    fn headers_for(origins: &[&str], origin: Option<&str>) -> header::HeaderMap {
        let cors = Cors::new(origins.iter().map(|s| s.to_string()).collect());
        let mut h = header::HeaderMap::new();
        cors.apply_headers(&mut h, origin);
        h
    }

    #[test]
    fn a_wildcard_configuration_allows_any_origin() {
        let h = headers_for(&["*"], Some("https://anywhere.example"));
        assert_eq!(
            h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "*",
            "в режиме '*' клиенту не нужно знать свой адрес"
        );
        assert!(h.get(header::ACCESS_CONTROL_ALLOW_METHODS).is_some());
    }

    #[test]
    fn a_wildcard_configuration_answers_even_without_an_origin_header() {
        // A non-browser client (curl, the Android app) sends no Origin; the
        // response must still be a valid CORS response.
        let h = headers_for(&["*"], None);
        assert_eq!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");
    }

    #[test]
    fn an_allow_list_echoes_the_requesting_origin() {
        let h = headers_for(&["https://app.example"], Some("https://app.example"));
        assert_eq!(
            h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
            "https://app.example"
        );
        // Without Vary the browser caches one response per URL and then serves
        // the wrong origin's answer to the next site.
        assert_eq!(h.get(header::VARY).unwrap(), "Origin");
    }

    #[test]
    fn an_allow_list_refuses_an_unknown_origin_entirely() {
        // Not "allow with the wrong origin": no headers at all, so the browser
        // blocks the response.
        let h = headers_for(&["https://app.example"], Some("https://evil.example"));
        assert!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        assert!(h.get(header::ACCESS_CONTROL_ALLOW_METHODS).is_none());
    }

    #[test]
    fn an_allow_list_without_an_origin_header_writes_nothing() {
        // Echoing a missing origin would produce an empty
        // Access-Control-Allow-Origin, which browsers treat as "denied".
        let h = headers_for(&["https://app.example"], None);
        assert!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }

    #[test]
    fn a_wildcard_in_a_longer_list_still_means_wildcard() {
        // A config of "https://a.example, *" is what a half-finished deployment
        // looks like; treating it as a literal would break the client.
        let h = headers_for(&["https://a.example", "*"], Some("https://b.example"));
        assert_eq!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");
    }

    #[test]
    fn an_origin_match_is_exact() {
        // A prefix match would let `https://app.example.evil.com` through.
        let cors = Cors::new(vec!["https://app.example".to_string()]);
        assert!(cors.allows(Some("https://app.example")));
        assert!(!cors.allows(Some("https://app.example.evil.com")));
        assert!(!cors.allows(Some("https://app.example/")));
        assert!(!cors.allows(None));
    }

    #[test]
    fn the_preflight_headers_cover_the_authenticated_endpoints() {
        // `authorization` has to be allowed or every authenticated call fails
        // the preflight in a browser.
        let h = headers_for(&["*"], Some("https://anywhere.example"));
        let methods = h
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .unwrap()
            .to_str()
            .unwrap();
        for m in ["GET", "POST", "PATCH", "DELETE", "OPTIONS"] {
            assert!(methods.contains(m), "метод {} не разрешён", m);
        }
        let hdrs = h
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .unwrap()
            .to_str()
            .unwrap();
        for x in ["authorization", "content-type", "x-admin-token"] {
            assert!(hdrs.contains(x), "заголовок {} не разрешён", x);
        }
        assert_eq!(h.get(header::ACCESS_CONTROL_MAX_AGE).unwrap(), "86400");
        assert!(h.get(header::ACCESS_CONTROL_EXPOSE_HEADERS).is_some());
    }

    #[test]
    fn an_origin_with_a_header_injection_attempt_is_not_echoed() {
        // HeaderValue::from_str rejects control characters, and the code drops
        // the header rather than trying to sanitise it into something wrong.
        let h = headers_for(
            &["https://good.example"],
            Some("https://good.example\r\nX-Evil: 1"),
        );
        assert!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    }
}
