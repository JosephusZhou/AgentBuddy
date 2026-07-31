//! Remove headers that could reveal the proxy infrastructure.
//! Reference: CLIProxyAPI ScrubProxyAndFingerprintHeaders.

use axum::http::HeaderMap;

/// Remove all proxy-tracing, client-identity, and browser-fingerprint headers.
/// Called before forwarding the request to the upstream provider.
pub fn scrub_proxy_headers(headers: &mut HeaderMap) {
    // Proxy tracing headers
    headers.remove("x-forwarded-for");
    headers.remove("x-forwarded-host");
    headers.remove("x-forwarded-proto");
    headers.remove("x-real-ip");
    headers.remove("via");
    headers.remove("forwarded");
    headers.remove("cf-connecting-ip");
    headers.remove("cf-ipcountry");
    headers.remove("cf-ray");
    headers.remove("cf-visitor");
    headers.remove("cf-worker");
    headers.remove("traceparent");
    headers.remove("tracestate");

    // Client identity headers — will be re-injected by cloaking
    headers.remove("x-stainless-retry-count");
    headers.remove("x-stainless-runtime");
    headers.remove("x-stainless-lang");
    headers.remove("x-stainless-timeout");
    headers.remove("x-stainless-package-version");
    headers.remove("x-stainless-runtime-version");
    headers.remove("x-stainless-os");
    headers.remove("x-stainless-arch");
    headers.remove("referer");

    // Browser fingerprint headers
    headers.remove("sec-ch-ua");
    headers.remove("sec-ch-ua-mobile");
    headers.remove("sec-ch-ua-platform");
    headers.remove("sec-fetch-mode");
    headers.remove("sec-fetch-site");
    headers.remove("sec-fetch-dest");
    headers.remove("sec-fetch-user");

    // Encoding negotiation — prevent zstd fingerprint mismatch
    headers.remove("accept-encoding");
}
