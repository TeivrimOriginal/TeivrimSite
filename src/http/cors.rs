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
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
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
            HeaderValue::from_static("authorization, content-type, x-admin-token, x-requested-with"),
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
