//! poly-ai against a fake OpenAI-compatible server on 127.0.0.1 (port 0).
//! No real model is needed, so this runs in CI.

use poly_ai::{AiClient, AiError, DEADLINE_REASON, LocalServer, Script, ServerKind};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MODEL: &str = "test-model";
const ORIGINAL: &str = "Hi Nino, I will not be able to come to the meeting at 18:30 on 17/10 because my train from Batumi is delayed. Please do not start without me.";
const GOOD: &str = "Nino, I will not make the 18:30 meeting on 17/10: my train from Batumi is delayed. Do not start without me.";
/// Drops the time 18:30, so the numbers check fails.
const BAD: &str = "Nino, I will not make the meeting on 17/10: my train from Batumi is delayed. Do not start without me.";
/// Short limits so the timing tests run fast.
const SHORT: Duration = Duration::from_millis(200);
const GENEROUS: Duration = Duration::from_secs(5);

#[derive(Clone)]
enum Reply {
    /// A chat answer with this content.
    Content(String),
    /// Wait, then answer with this content.
    Delayed(Duration, String),
    /// An HTTP error status.
    Status(u16),
    /// 200 with a body that isn't JSON.
    Garbage,
}

struct FakeServer {
    base_url: String,
    /// Bodies of the chat requests received, in order.
    requests: Arc<Mutex<Vec<Value>>>,
}

impl FakeServer {
    fn start(replies: Vec<Reply>) -> Self {
        Self::start_with_models_delay(replies, Duration::ZERO)
    }

    fn start_with_models_delay(replies: Vec<Reply>, models_delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let (replies, recorded) = (replies.clone(), recorded.clone());
                thread::spawn(move || handle(stream, &replies, &recorded, models_delay));
            }
        });
        FakeServer {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            requests,
        }
    }

    fn server(&self) -> LocalServer {
        LocalServer {
            kind: ServerKind::Custom,
            base_url: self.base_url.clone(),
            models: vec![MODEL.into()],
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }
}

fn handle(
    stream: TcpStream,
    replies: &Mutex<VecDeque<Reply>>,
    recorded: &Mutex<Vec<Value>>,
    models_delay: Duration,
) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).unwrap_or_default();

    let (status, out) = if request_line.starts_with("GET /v1/models") {
        thread::sleep(models_delay);
        (200, json!({ "data": [{ "id": MODEL }] }).to_string())
    } else if request_line.starts_with("POST /v1/chat/completions") {
        recorded
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
        let reply = replies.lock().unwrap().pop_front();
        let answer =
            |c: String| json!({ "choices": [{ "message": { "content": c } }] }).to_string();
        match reply {
            Some(Reply::Content(c)) => (200, answer(c)),
            Some(Reply::Delayed(d, c)) => {
                thread::sleep(d);
                (200, answer(c))
            }
            Some(Reply::Status(s)) => (s, "{}".into()),
            Some(Reply::Garbage) => (200, "not json".into()),
            None => (500, "{}".into()),
        }
    } else {
        (404, "{}".into())
    };
    // Redirects point at another local port; following one would be a bug.
    let location = if (300..400).contains(&status) {
        "Location: http://127.0.0.1:9/v1/chat/completions\r\n"
    } else {
        ""
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} X\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
}

fn client() -> AiClient {
    AiClient::with_timeouts(GENEROUS, GENEROUS, GENEROUS)
}

fn user_message(request: &Value) -> String {
    request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn probe_lists_models() {
    let fake = FakeServer::start(vec![]);
    let server = client().probe(ServerKind::Custom, &fake.base_url).unwrap();
    assert_eq!(server.models, vec![MODEL]);
}

#[test]
fn probe_rejects_non_local_url() {
    assert!(matches!(
        client().probe(ServerKind::Custom, "http://192.168.1.5:1234/v1"),
        Err(AiError::NotLocal(_))
    ));
}

#[test]
fn shorten_success() {
    let fake = FakeServer::start(vec![Reply::Content(GOOD.into())]);
    let s = client().shorten(&fake.server(), MODEL, ORIGINAL).unwrap();
    assert_eq!(s.text, GOOD);
    assert_eq!(
        (s.original_bytes, s.new_bytes),
        (ORIGINAL.len(), GOOD.len())
    );
    assert!(s.saved_percent > 0.0);
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], MODEL);
    assert_eq!(requests[0]["temperature"], 0.1);
    assert_eq!(requests[0]["stream"], false);
    assert!(user_message(&requests[0]).contains(ORIGINAL));
}

#[test]
fn retry_after_bad_answer_includes_feedback_and_higher_temperature() {
    let fake = FakeServer::start(vec![
        Reply::Content(BAD.into()),
        Reply::Content(GOOD.into()),
    ]);
    let s = client().shorten(&fake.server(), MODEL, ORIGINAL).unwrap();
    assert_eq!(s.text, GOOD);
    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert!(!user_message(&requests[0]).contains("previous attempt"));
    let retry = user_message(&requests[1]);
    assert!(retry.contains("previous attempt was rejected"), "{retry}");
    assert!(retry.contains("\"18:30\""), "{retry}");
    assert_eq!(requests[1]["temperature"], 0.3);
}

#[test]
fn rejected_after_last_attempt() {
    let fake = FakeServer::start(vec![Reply::Content(BAD.into()); 3]);
    let err = client()
        .shorten(&fake.server(), MODEL, ORIGINAL)
        .unwrap_err();
    assert_eq!(
        err,
        AiError::Rejected {
            reasons: vec!["numbers".into()]
        }
    );
    assert_eq!(fake.requests().len(), 3);
}

#[test]
fn think_tags_are_removed() {
    let fake = FakeServer::start(vec![Reply::Content(format!(
        "<think>The user wants it shorter. Keep 18:30.</think>\n\n{GOOD}"
    ))]);
    let s = client().shorten(&fake.server(), MODEL, ORIGINAL).unwrap();
    assert_eq!(s.text, GOOD);
}

#[test]
fn timeout_counts_as_failed_attempt_and_is_retried() {
    let fake = FakeServer::start(vec![
        Reply::Delayed(SHORT * 3, GOOD.into()),
        Reply::Content(GOOD.into()),
    ]);
    let ai = AiClient::with_timeouts(GENEROUS, SHORT, GENEROUS);
    let s = ai.shorten(&fake.server(), MODEL, ORIGINAL).unwrap();
    assert_eq!(s.text, GOOD);
    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert!(user_message(&requests[1]).contains("took too long"));
}

#[test]
fn all_timeouts_are_rejected_with_reason() {
    let fake = FakeServer::start(vec![Reply::Delayed(SHORT * 3, GOOD.into()); 3]);
    let ai = AiClient::with_timeouts(GENEROUS, SHORT, GENEROUS);
    let err = ai.shorten(&fake.server(), MODEL, ORIGINAL).unwrap_err();
    assert_eq!(
        err,
        AiError::Rejected {
            reasons: vec!["timeout".into()]
        }
    );
}

#[test]
fn unusable_answer_is_retried() {
    let fake = FakeServer::start(vec![Reply::Garbage, Reply::Content(GOOD.into())]);
    let s = client().shorten(&fake.server(), MODEL, ORIGINAL).unwrap();
    assert_eq!(s.text, GOOD);
    assert_eq!(fake.requests().len(), 2);
}

#[test]
fn error_status_stops_without_retry() {
    let fake = FakeServer::start(vec![Reply::Status(404), Reply::Content(GOOD.into())]);
    let err = client()
        .shorten(&fake.server(), MODEL, ORIGINAL)
        .unwrap_err();
    assert!(matches!(err, AiError::Server(_)), "{err:?}");
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn redirects_are_not_followed() {
    let fake = FakeServer::start(vec![Reply::Status(302), Reply::Content(GOOD.into())]);
    let err = client()
        .shorten(&fake.server(), MODEL, ORIGINAL)
        .unwrap_err();
    assert_eq!(err, AiError::Server("HTTP status 302".into()));
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn dead_server_stops_without_retry() {
    // Bind to get a free port, then close it: nothing listens there.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let server = LocalServer {
        kind: ServerKind::Custom,
        base_url: format!("http://127.0.0.1:{port}/v1"),
        models: vec![MODEL.into()],
    };
    let start = Instant::now();
    let err = client().shorten(&server, MODEL, ORIGINAL).unwrap_err();
    assert_eq!(err, AiError::NoLocalServer);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(matches!(
        client().probe(ServerKind::Custom, &server.base_url),
        Err(AiError::NoLocalServer)
    ));
}

#[test]
fn deadline_is_enforced() {
    // Each answer is bad and takes 300 ms; the deadline is 500 ms. Attempt 1
    // fails at ~300 ms, attempt 2 is cut off at the deadline, attempt 3 never starts.
    let delay = Duration::from_millis(300);
    let deadline = Duration::from_millis(500);
    let fake = FakeServer::start(vec![Reply::Delayed(delay, BAD.into()); 3]);
    let ai = AiClient::with_timeouts(GENEROUS, GENEROUS, deadline);
    let start = Instant::now();
    let err = ai.shorten(&fake.server(), MODEL, ORIGINAL).unwrap_err();
    let took = start.elapsed();
    let AiError::Rejected { reasons } = err else {
        panic!("{err:?}")
    };
    assert!(
        reasons.contains(&DEADLINE_REASON.to_string()),
        "{reasons:?}"
    );
    assert_eq!(fake.requests().len(), 2);
    assert!(took < deadline + SHORT, "{took:?}");
}

#[test]
fn discovery_times_out_fast() {
    let fake = FakeServer::start_with_models_delay(vec![], SHORT * 3);
    let ai = AiClient::with_timeouts(SHORT, GENEROUS, GENEROUS);
    let start = Instant::now();
    assert_eq!(
        ai.probe(ServerKind::Custom, &fake.base_url),
        Err(AiError::Timeout)
    );
    assert!(start.elapsed() < SHORT * 3);
}

#[test]
fn decoy_that_fits() {
    // Target 40 bytes pads to 48 (poly-core): 31..=46 bytes fit.
    let decoy = "Did you buy milk on the way home today?";
    let fake = FakeServer::start(vec![Reply::Content(decoy.into())]);
    let d = client()
        .generate_decoy(
            &fake.server(),
            MODEL,
            40,
            Script::Latin,
            Some("casual".into()),
        )
        .unwrap();
    assert!(d.fits);
    assert_eq!(d.text, decoy);
    assert_eq!(d.allowed_len_range, (31, 46));
    let requests = fake.requests();
    let system = requests[0]["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("English") && system.contains("40 bytes") && system.contains("casual"));
    assert_eq!(requests[0]["temperature"], 0.8);
}

#[test]
fn decoy_padding_mismatch_returns_closest_with_fits_false() {
    let far = "Ok.";
    let closer = "See you at the shop later.";
    let fake = FakeServer::start(vec![
        Reply::Content(far.into()),
        Reply::Content(closer.into()),
        Reply::Content("My secret plan for the weekend is ready.".into()),
    ]);
    let d = client()
        .generate_decoy(&fake.server(), MODEL, 40, Script::Latin, None)
        .unwrap();
    assert!(!d.fits);
    assert_eq!(d.text, closer);
    assert_eq!(d.allowed_len_range, (31, 46));
    let requests = fake.requests();
    assert_eq!(requests.len(), 3);
    let retry = user_message(&requests[1]);
    assert!(retry.contains("between 31 and 46 bytes"), "{retry}");
    assert_eq!(requests[1]["temperature"], 0.9);
}

#[test]
fn decoy_rejected_when_nothing_passes() {
    let fake = FakeServer::start(vec![
        Reply::Content("Here is an everyday message: hi".into());
        3
    ]);
    let err = client()
        .generate_decoy(&fake.server(), MODEL, 40, Script::Latin, None)
        .unwrap_err();
    let AiError::Rejected { reasons } = err else {
        panic!("{err:?}")
    };
    assert!(
        reasons.contains(&"preamble".to_string()) && reasons.contains(&"prompt_echo".to_string())
    );
}
