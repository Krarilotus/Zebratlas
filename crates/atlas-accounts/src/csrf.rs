//! Browser mutation guard, also applied around the complete server router.
//! A custom header cannot be set by a cross-origin form. Cookie clients must send it;
//! bearer clients do not use ambient credentials. Cross-site browser calls are rejected
//! even without cookies (login CSRF and anonymous paid-resource abuse).
use axum::extract::Request;
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AccountsError;

pub fn allowed(method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    if let Some(site) = headers.get("sec-fetch-site")
        && site != "same-origin"
        && site != "none"
    {
        return false;
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let same = origin
            .to_str()
            .ok()
            .and_then(|o| o.parse::<axum::http::Uri>().ok())
            .filter(|o| matches!(o.scheme_str(), Some("https" | "http")))
            .and_then(|o| o.authority().map(|a| a.as_str().to_owned()))
            .zip(headers.get(header::HOST).and_then(|h| h.to_str().ok()))
            .is_some_and(|(a, h)| a.eq_ignore_ascii_case(h));
        if !same {
            return false;
        }
    }
    !headers.contains_key(header::COOKIE) || headers.get("x-atlas-csrf").is_some_and(|v| v == "1")
}

pub async fn guard(request: Request, next: Next) -> Response {
    if !allowed(request.method(), request.headers()) {
        return AccountsError::Forbidden.into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutations_require_non_simple_header_and_reject_sibling_origins() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "__Host-atlas_session=token".parse().unwrap());
        h.insert(header::HOST, "atlas.example".parse().unwrap());
        for method in [Method::POST, Method::PATCH, Method::PUT, Method::DELETE] {
            assert!(!allowed(&method, &h));
        }
        assert!(allowed(&Method::GET, &h));
        h.insert("x-atlas-csrf", "1".parse().unwrap());
        assert!(allowed(&Method::POST, &h));
        h.insert(header::ORIGIN, "https://atlas.example".parse().unwrap());
        assert!(allowed(&Method::POST, &h));
        for origin in ["https://evil.atlas.example", "https://atlas.example.evil", "null"] {
            h.insert(header::ORIGIN, origin.parse().unwrap());
            assert!(!allowed(&Method::POST, &h));
        }
        h.remove(header::ORIGIN);
        h.insert("sec-fetch-site", "same-site".parse().unwrap());
        assert!(!allowed(&Method::POST, &h));
        h.clear();
        h.insert(header::AUTHORIZATION, "Bearer token".parse().unwrap());
        assert!(allowed(&Method::POST, &h));
        h.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(!allowed(&Method::POST, &h));
    }
}
