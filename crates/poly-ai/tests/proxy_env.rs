//! Proxy environment variables are ignored. Its own test binary, so setting
//! the variables can't race other tests.

use poly_ai::{AiClient, ServerKind};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

/// Plenty for a loopback round trip.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Enough to hold a whole GET request with headers.
const REQUEST_BUFFER: usize = 4096;

#[test]
fn proxy_variables_are_ignored() {
    // A "proxy" that would answer with a different model list.
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1", server.local_addr().unwrap());
    for (listener, model) in [(proxy, "via-proxy"), (server, "direct")] {
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0; REQUEST_BUFFER];
                let _ = stream.read(&mut buf);
                let body = format!(r#"{{"data":[{{"id":"{model}"}}]}}"#);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
    }
    for var in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        // SAFETY: this test binary has a single test, so no other thread reads the environment.
        unsafe { std::env::set_var(var, &proxy_url) };
    }
    let ai = AiClient::with_timeouts(TIMEOUT, TIMEOUT, TIMEOUT);
    let found = ai.probe(ServerKind::Custom, &base_url).unwrap();
    assert_eq!(found.models, vec!["direct"]);
}
