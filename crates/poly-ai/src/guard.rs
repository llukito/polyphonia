//! The localhost guarantee: only plain-http URLs whose host is a literal
//! loopback address are ever contacted.
//!
//! `localhost` is mapped to `127.0.0.1` here, without a DNS lookup, so a
//! tampered resolver or hosts file can't redirect us. Everything else
//! (LAN addresses, names, https to anywhere) is rejected.

use crate::AiError;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Port used when the URL names none (the http default, RFC 9110 §4.2.1).
const HTTP_DEFAULT_PORT: u16 = 80;
const SCHEME: &str = "http://";
/// The only host name we accept; it's mapped to 127.0.0.1 without DNS.
const LOCALHOST: &str = "localhost";

/// Check a base URL (e.g. `http://localhost:1234/v1`) and return it
/// normalized with a literal IP host and explicit port, e.g.
/// `http://127.0.0.1:1234/v1`. The normalized form is what gets requested,
/// so no name is ever resolved.
pub fn validate_base_url(url: &str) -> Result<String, AiError> {
    let reject = |why: &str| Err(AiError::NotLocal(why.to_string()));

    if !url
        .get(..SCHEME.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(SCHEME))
    {
        return reject("only plain http:// to this computer is allowed");
    }
    let rest = &url[SCHEME.len()..];
    if rest.contains(['?', '#']) || rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return reject("query, fragment or whitespace in the URL");
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.contains('@') {
        return reject("user info in the URL");
    }

    let (host, port) = if let Some(after_bracket) = authority.strip_prefix('[') {
        let Some((inside, after)) = after_bracket.split_once(']') else {
            return reject("bad IPv6 address");
        };
        let Ok(ip) = inside.parse::<Ipv6Addr>() else {
            return reject("bad IPv6 address");
        };
        if !ip.is_loopback() {
            return reject("not a loopback address");
        }
        let port = match after {
            "" => None,
            p => match p.strip_prefix(':') {
                Some(p) => Some(p),
                None => return reject("bad port"),
            },
        };
        (format!("[{ip}]"), port)
    } else {
        let (name, port) = match authority.split_once(':') {
            Some((n, p)) => (n, Some(p)),
            None => (authority, None),
        };
        let ip = if name.eq_ignore_ascii_case(LOCALHOST) {
            Ipv4Addr::LOCALHOST
        } else {
            match name.parse::<Ipv4Addr>() {
                Ok(ip) if ip.is_loopback() => ip,
                Ok(_) => return reject("not a loopback address"),
                Err(_) => return reject("host must be localhost or a loopback IP"),
            }
        };
        (ip.to_string(), port)
    };

    let port = match port {
        None => HTTP_DEFAULT_PORT,
        Some(p) => match p.parse::<u16>() {
            Ok(p) if p != 0 => p,
            _ => return reject("bad port"),
        },
    };
    let path = path.trim_end_matches('/');
    if !path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._~/".contains(c))
    {
        return reject("unexpected characters in the path");
    }
    Ok(format!("{SCHEME}{host}:{port}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback() {
        let ok = |u: &str| validate_base_url(u).unwrap();
        assert_eq!(ok("http://127.0.0.1:1234/v1"), "http://127.0.0.1:1234/v1");
        assert_eq!(
            ok("http://127.8.9.10:11434/v1/"),
            "http://127.8.9.10:11434/v1"
        );
        assert_eq!(ok("http://[::1]:1234/v1"), "http://[::1]:1234/v1");
        assert_eq!(ok("http://localhost:11434/v1"), "http://127.0.0.1:11434/v1");
        assert_eq!(ok("HTTP://LOCALHOST/v1"), "http://127.0.0.1:80/v1");
    }

    #[test]
    fn rejects_everything_else() {
        for url in [
            "http://192.168.1.5:1234/v1",
            "http://example.com/v1",
            "http://localhost.evil.com:1234/v1",
            "http://evil.localhost:1234/v1",
            "https://127.0.0.1:1234/v1",
            "https://api.openai.com/v1",
            "http://127.0.0.1@evil.com/v1",
            "http://evil.com@127.0.0.1/v1",
            "http://[::ffff:127.0.0.1]:1234/v1",
            "http://[fe80::1]:1234/v1",
            "http://0.0.0.0:1234/v1",
            "http://127.1:1234/v1",
            "http://127.0.0.1:0/v1",
            "http://127.0.0.1:99999/v1",
            "http://127.0.0.1:1234/v1?x=1",
            "http://127.0.0.1:1234/v 1",
            "ftp://127.0.0.1/v1",
            "127.0.0.1:1234",
            "",
        ] {
            assert!(
                matches!(validate_base_url(url), Err(AiError::NotLocal(_))),
                "{url}"
            );
        }
    }
}
