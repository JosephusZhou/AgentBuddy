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

/// Sanitized passthrough of genuine client request headers.
///
/// 用于客户端本身就是目标 CLI（如真实 Claude Code / Codex CLI）的场景：
/// 客户端头即最真实的指纹，应原样转发给上游（与"直连该供应商"形态一致），
/// 仅剔除会暴露代理链或由转发层重新生成的头：
/// - 鉴权头（authorization / x-api-key / cookie）——由 provider 密钥重新注入；
/// - hop-by-hop 与传输控制（host / content-length / content-type /
///   transfer-encoding / connection）——由 HTTP 客户端重算；
/// - 代理追踪头（x-forwarded-* / via / cf-* 等）——见 scrub_proxy_headers；
/// - accept-encoding——交给 reqwest 重新协商，避免解压错配。
///
/// 注意：与 scrub_proxy_headers 不同，这里**保留** x-stainless-* 等真实 SDK
/// 指纹头——它们来自真实客户端，剥掉反而使请求偏离直连形态。
pub fn passthrough_client_headers(client_headers: &HeaderMap) -> HeaderMap {
    let mut headers = client_headers.clone();
    // Auth — re-injected per provider by forwarder::build_auth_headers.
    headers.remove("authorization");
    headers.remove("x-api-key");
    headers.remove("cookie");
    // Hop-by-hop / transport-controlled — recomputed by reqwest or skipped in
    // send_request's copy loop.
    headers.remove("host");
    headers.remove("content-length");
    headers.remove("content-type");
    headers.remove("transfer-encoding");
    headers.remove("connection");
    // Proxy tracing — must never leak the local proxy hop.
    headers.remove("x-forwarded-for");
    headers.remove("x-forwarded-host");
    headers.remove("x-forwarded-proto");
    headers.remove("x-real-ip");
    headers.remove("via");
    headers.remove("forwarded");
    for name in [
        "cf-connecting-ip",
        "cf-ipcountry",
        "cf-ray",
        "cf-visitor",
        "cf-worker",
    ] {
        headers.remove(name);
    }
    headers.remove("traceparent");
    headers.remove("tracestate");
    // Re-negotiated by reqwest.
    headers.remove("accept-encoding");
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn hv(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    #[test]
    fn passthrough_keeps_client_fingerprints_and_strips_auth_and_proxies() {
        let mut client = HeaderMap::new();
        client.insert("user-agent", hv("claude-cli/2.1.237 (external, cli)"));
        client.insert("authorization", hv("Bearer sk-local-route-key"));
        client.insert("x-api-key", hv("sk-local-route-key"));
        client.insert("cookie", hv("session=1"));
        client.insert("anthropic-beta", hv("claude-code-20250219"));
        client.insert("x-app", hv("cli"));
        client.insert("x-stainless-lang", hv("js"));
        client.insert("accept", hv("application/json"));
        client.insert("via", hv("1.1 proxy"));
        client.insert("x-forwarded-for", hv("10.0.0.1"));
        client.insert("accept-encoding", hv("gzip, br, zstd"));
        client.insert("host", hv("127.0.0.1:16888"));
        client.insert("content-length", hv("123"));

        let out = passthrough_client_headers(&client);

        assert_eq!(
            out.get("user-agent").unwrap(),
            "claude-cli/2.1.237 (external, cli)"
        );
        assert!(out.get("anthropic-beta").is_some());
        assert!(out.get("x-app").is_some());
        assert!(out.get("x-stainless-lang").is_some());
        assert_eq!(out.get("accept").unwrap(), "application/json");
        for stripped in [
            "authorization",
            "x-api-key",
            "cookie",
            "via",
            "x-forwarded-for",
            "accept-encoding",
            "host",
            "content-length",
        ] {
            assert!(out.get(stripped).is_none(), "{stripped} 应被剔除");
        }
    }
}
