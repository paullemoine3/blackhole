//! The alias services' HTTP APIs (SimpleLogin, Addy.io, Firefox Relay). Blocking: call off the UI
//! thread. The key goes in a header to the account's own server only (no redirect followed), and
//! nothing is sent but what the call needs; the site's name only when the caller gives it.

use std::time::Duration;

use blackhole_domain::aliases::{valid_email, valid_remote, Account, Provider};
use blackhole_domain::i18n::message;
use serde_json::{json, Value};

const MAX_BYTES: u64 = 512 * 1024;

/// A new alias, as the service answered.
#[derive(Debug, PartialEq, Eq)]
pub struct Made {
    pub remote: String,
    pub email: String,
}

enum Failure {
    Status(u16, String),
    BadAnswer,
    Unreachable(String),
}

impl Failure {
    /// What to show. A service's own words (quota reached…) are kept short, on one line.
    fn text(self) -> String {
        match self {
            Self::Status(401 | 403, _) => message("msg.alias_key_refused", &[]),
            Self::Status(429, _) => message("msg.alias_too_many", &[]),
            Self::Status(code, why) => {
                let why: String = why.chars().filter(|c| !c.is_control()).take(160).collect();
                let why = if why.trim().is_empty() { format!("HTTP {code}") } else { why };
                message("msg.alias_refused", &[("why", &why)])
            }
            Self::BadAnswer => message("msg.alias_bad_answer", &[]),
            Self::Unreachable(why) => message("msg.alias_unreachable", &[("why", &why)]),
        }
    }
}

fn call(account: &Account, method: &str, path: &str, body: Option<&Value>) -> Result<Value, Failure> {
    let agent = ureq::AgentBuilder::new()
        .user_agent(concat!("BlackHole/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .redirects(0)
        .build();
    let request = agent
        .request(method, &format!("{}{path}", account.server))
        .set("Accept", "application/json");
    let request = match account.provider {
        Provider::SimpleLogin => request.set("Authentication", &account.key),
        Provider::Addy => request
            .set("Authorization", &format!("Bearer {}", account.key))
            .set("X-Requested-With", "XMLHttpRequest"),
        Provider::FirefoxRelay => request.set("Authorization", &format!("Token {}", account.key)),
    };
    let result = match body {
        Some(b) => request.set("Content-Type", "application/json").send_string(&b.to_string()),
        None => request.call(),
    };
    let read = |r: ureq::Response| crate::updater::read_limited(r, MAX_BYTES);
    match result {
        // Without redirects followed, ureq hands a 3xx over as an answer.
        Ok(r) if r.status() >= 300 => Err(Failure::Status(r.status(), String::new())),
        Ok(r) => {
            let bytes = read(r).map_err(Failure::Unreachable)?;
            if bytes.iter().all(u8::is_ascii_whitespace) {
                return Ok(Value::Null);
            }
            serde_json::from_slice(&bytes).map_err(|_| Failure::BadAnswer)
        }
        Err(ureq::Error::Status(code, r)) => {
            let value: Value = read(r)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or(Value::Null);
            let why = ["error", "message", "detail"]
                .iter()
                .find_map(|k| value[*k].as_str())
                .unwrap_or("");
            Err(Failure::Status(code, why.to_owned()))
        }
        Err(ureq::Error::Transport(t)) => Err(Failure::Unreachable(t.kind().to_string())),
    }
}

/// The key is accepted by the service. Nothing is created.
pub fn check(account: &Account) -> Result<(), String> {
    let path = match account.provider {
        Provider::SimpleLogin => "/api/user_info",
        Provider::Addy => "/api/v1/api-token-details",
        Provider::FirefoxRelay => "/api/v1/relayaddresses/",
    };
    let answer = call(account, "GET", path, None).map_err(Failure::text)?;
    // A page that answers 200 to anything is not the service.
    if answer.is_object() || answer.is_array() {
        Ok(())
    } else {
        Err(Failure::BadAnswer.text())
    }
}

/// The domain Addy.io makes aliases on: the account's own choice, else the first it may use.
fn addy_domain(account: &Account) -> Result<String, Failure> {
    let options = call(account, "GET", "/api/v1/domain-options", None)?;
    options["defaultAliasDomain"]
        .as_str()
        .or_else(|| options["data"].as_array()?.first()?.as_str())
        .map(str::to_owned)
        .ok_or(Failure::BadAnswer)
}

fn made(value: &Value, email: &str) -> Result<Made, Failure> {
    let remote = match &value["id"] {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    let email = value[email].as_str().unwrap_or_default().to_owned();
    if valid_remote(&remote) && valid_email(&email) {
        Ok(Made { remote, email })
    } else {
        Err(Failure::BadAnswer)
    }
}

/// Makes an alias. `site`: the host written beside it on the service, only if the user said so.
pub fn create(account: &Account, site: Option<&str>) -> Result<Made, String> {
    let run = || match account.provider {
        Provider::SimpleLogin => {
            let body = site.map_or_else(|| json!({}), |s| json!({ "note": s }));
            let v = call(account, "POST", "/api/alias/random/new?mode=word", Some(&body))?;
            // `alias` in the older answers, `email` in the documented one.
            let field = if v["email"].is_string() { "email" } else { "alias" };
            made(&v, field)
        }
        Provider::Addy => {
            let mut body = json!({ "domain": addy_domain(account)?, "format": "random_characters" });
            if let Some(s) = site {
                body["description"] = json!(s);
            }
            made(&call(account, "POST", "/api/v1/aliases", Some(&body))?["data"], "email")
        }
        Provider::FirefoxRelay => {
            let mut body = json!({ "enabled": true });
            if let Some(s) = site {
                body["description"] = json!(s);
                body["generated_for"] = json!(s);
            }
            made(&call(account, "POST", "/api/v1/relayaddresses/", Some(&body))?, "full_address")
        }
    };
    run().map_err(Failure::text)
}

fn safe(remote: &str) -> Result<(), String> {
    valid_remote(remote).then_some(()).ok_or_else(|| Failure::BadAnswer.text())
}

/// Switches an alias on or off.
pub fn set_enabled(account: &Account, remote: &str, on: bool) -> Result<(), String> {
    safe(remote)?;
    let run = || match account.provider {
        Provider::SimpleLogin => {
            // The API only flips: an alias changed on the service's site meanwhile is flipped back.
            let path = format!("/api/aliases/{remote}/toggle");
            match call(account, "POST", &path, None)?["enabled"].as_bool() {
                Some(now) if now != on => call(account, "POST", &path, None).map(drop),
                Some(_) => Ok(()),
                None => Err(Failure::BadAnswer),
            }
        }
        Provider::Addy if on => {
            call(account, "POST", "/api/v1/active-aliases", Some(&json!({ "id": remote }))).map(drop)
        }
        Provider::Addy => {
            call(account, "DELETE", &format!("/api/v1/active-aliases/{remote}"), None).map(drop)
        }
        Provider::FirefoxRelay => {
            let path = format!("/api/v1/relayaddresses/{remote}/");
            call(account, "PATCH", &path, Some(&json!({ "enabled": on }))).map(drop)
        }
    };
    run().map_err(Failure::text)
}

/// Deletes an alias; one the service no longer has counts as deleted.
pub fn delete(account: &Account, remote: &str) -> Result<(), String> {
    safe(remote)?;
    let path = match account.provider {
        Provider::SimpleLogin => format!("/api/aliases/{remote}"),
        Provider::Addy => format!("/api/v1/aliases/{remote}"),
        Provider::FirefoxRelay => format!("/api/v1/relayaddresses/{remote}/"),
    };
    match call(account, "DELETE", &path, None) {
        Ok(_) | Err(Failure::Status(404, _)) => Ok(()),
        Err(e) => Err(e.text()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    type Replies = &'static [(&'static str, &'static str)];

    /// Answers `replies` in turn and returns the requests received (headers and body).
    fn serve(replies: Replies) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = format!("http://{}", listener.local_addr().unwrap());
        let run = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut text, mut length) = (String::new(), 0);
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    if line.trim().is_empty() {
                        break;
                    }
                    text.push_str(&line);
                }
                let mut b = vec![0; length];
                reader.read_exact(&mut b).unwrap();
                text.push_str(&String::from_utf8(b).unwrap());
                seen.push(text);
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: /x\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
            }
            seen
        });
        (server, run)
    }

    fn account(provider: Provider, server: &str) -> Account {
        Account {
            provider,
            server: server.into(),
            key: "key-test".into(),
        }
    }

    #[test]
    fn simplelogin_creates_without_the_site_unless_asked() {
        let (s, run) = serve(&[
            ("201 Created", r#"{"id":7,"email":"a.b@sl.test","enabled":true}"#),
            ("201 Created", r#"{"id":8,"email":"c@sl.test"}"#),
        ]);
        let a = account(Provider::SimpleLogin, &s);
        let first = create(&a, None).unwrap();
        assert_eq!((first.remote.as_str(), first.email.as_str()), ("7", "a.b@sl.test"));
        create(&a, Some("shop.test")).unwrap();
        let seen = run.join().unwrap();
        assert!(seen[0].starts_with("POST /api/alias/random/new?mode=word "));
        assert!(seen[0].contains("Authentication: key-test"));
        assert!(!seen[0].contains("shop.test") && seen[1].contains("shop.test"));
    }

    #[test]
    fn simplelogin_flips_again_when_the_state_was_not_the_expected_one() {
        let (s, run) = serve(&[("200 OK", r#"{"enabled":true}"#), ("200 OK", r#"{"enabled":false}"#)]);
        set_enabled(&account(Provider::SimpleLogin, &s), "7", false).unwrap();
        assert_eq!(run.join().unwrap().len(), 2);
    }

    #[test]
    fn addy_uses_the_account_domain_and_reads_the_data() {
        let (s, run) = serve(&[
            ("200 OK", r#"{"data":["x.test"],"defaultAliasDomain":"mine.test"}"#),
            ("201 Created", r#"{"data":{"id":"50c9-ab","email":"q@mine.test"}}"#),
            ("204 No Content", ""),
        ]);
        let a = account(Provider::Addy, &s);
        assert_eq!(create(&a, None).unwrap().email, "q@mine.test");
        set_enabled(&a, "50c9-ab", false).unwrap();
        let seen = run.join().unwrap();
        assert!(seen[1].contains("mine.test") && seen[1].contains("Authorization: Bearer key-test"));
        assert!(seen[2].starts_with("DELETE /api/v1/active-aliases/50c9-ab "));
    }

    #[test]
    fn relay_reads_the_full_address() {
        let (s, run) = serve(&[
            ("201 Created", r#"{"id":3,"full_address":"z@mozmail.test"}"#),
            ("200 OK", "{}"),
        ]);
        let a = account(Provider::FirefoxRelay, &s);
        assert_eq!(create(&a, None).unwrap().email, "z@mozmail.test");
        set_enabled(&a, "3", false).unwrap();
        let seen = run.join().unwrap();
        assert!(seen[0].contains("Authorization: Token key-test"));
        assert!(seen[1].starts_with("PATCH /api/v1/relayaddresses/3/ ") && seen[1].contains("false"));
    }

    #[test]
    fn refusals_are_told_and_a_redirect_never_takes_the_key_elsewhere() {
        let (s, run) = serve(&[
            ("401 Unauthorized", "{}"),
            ("400 Bad Request", r#"{"error":"quota reached"}"#),
            ("302 Found", ""),
            ("404 Not Found", "{}"),
            ("200 OK", r#"{"id":1,"email":"<b>@x"}"#),
            ("200 OK", "<html>"),
        ]);
        let a = account(Provider::SimpleLogin, &s);
        assert_eq!(check(&a), Err(message("msg.alias_key_refused", &[])));
        assert_eq!(
            create(&a, None),
            Err(message("msg.alias_refused", &[("why", "quota reached")]))
        );
        assert!(check(&a).is_err(), "a redirect is refused");
        assert_eq!(delete(&a, "9"), Ok(()), "already gone");
        assert!(create(&a, None).is_err(), "not an address");
        assert!(check(&a).is_err(), "not the service");
        assert_eq!(run.join().unwrap().len(), 6);
        assert!(delete(&a, "../x").is_err());
    }
}
