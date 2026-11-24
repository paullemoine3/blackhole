//! The alias services' HTTP APIs (SimpleLogin, Addy.io, Firefox Relay, Fastmail,
//! Forward Email). Blocking: call off the UI
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
    NoDomain,
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
            Self::NoDomain => message("msg.alias_no_domain", &[]),
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
        Provider::Fastmail => request.set("Authorization", &format!("Bearer {}", account.key)),
        // Basic authentication: the key is the user, without a password.
        Provider::ForwardEmail => {
            request.set("Authorization", &format!("Basic {}", base64(format!("{}:", account.key).as_bytes())))
        }
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

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            out.push(if i <= chunk.len() { TABLE[(n >> (18 - 6 * i)) as usize & 63] as char } else { '=' });
        }
    }
    out
}

const JMAP_MASKED: &str = "https://www.fastmail.com/dev/maskedemail";

/// Fastmail's account id, read from its JMAP session. The calls then go to the account's own
/// server, never to an address the session would name.
fn fastmail_account(account: &Account) -> Result<String, Failure> {
    let session = call(account, "GET", "/jmap/session", None)?;
    match session["primaryAccounts"][JMAP_MASKED].as_str() {
        Some(id) if valid_remote(id) => Ok(id.to_owned()),
        // A token without the "Masked Email" scope.
        Some(_) => Err(Failure::BadAnswer),
        None => Err(Failure::Status(403, String::new())),
    }
}

/// One `MaskedEmail/set`. `part` (`created` / `updated`) must name `id` in the answer.
fn fastmail_set(account: &Account, change: &Value, part: &str, id: &str) -> Result<Value, Failure> {
    let mut args = change.clone();
    args["accountId"] = json!(fastmail_account(account)?);
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", JMAP_MASKED],
        "methodCalls": [["MaskedEmail/set", args, "0"]],
    });
    let answer = call(account, "POST", "/jmap/api/", Some(&body))?;
    let result = &answer["methodResponses"][0][1];
    if let Some(done) = result[part].as_object().and_then(|m| m.get(id)) {
        return Ok(done.clone());
    }
    let refused = ["notCreated", "notUpdated"].iter().map(|k| &result[*k][id]).find(|e| e.is_object());
    match refused {
        Some(e) => {
            let why = e["description"].as_str().or_else(|| e["type"].as_str()).unwrap_or("");
            Err(Failure::Status(400, why.to_owned()))
        }
        None => Err(Failure::BadAnswer),
    }
}

/// The first domain of a Forward Email account: aliases live on the user's own domains.
fn forward_domain(account: &Account) -> Result<String, Failure> {
    let domains = call(account, "GET", "/v1/domains", None)?;
    let name = domains
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["name"].as_str()))
        .ok_or(Failure::NoDomain)?;
    // Goes into an address path.
    if valid_email(&format!("a@{name}")) {
        Ok(name.to_owned())
    } else {
        Err(Failure::BadAnswer)
    }
}

/// The address path of a Forward Email alias, and the name it must keep.
fn forward_path<'a>(remote: &str, email: &'a str) -> Result<(String, &'a str), Failure> {
    match email.split_once('@') {
        Some((name, domain)) if valid_email(email) => Ok((format!("/v1/domains/{domain}/aliases/{remote}"), name)),
        _ => Err(Failure::BadAnswer),
    }
}

/// The key is accepted by the service. Nothing is created.
pub fn check(account: &Account) -> Result<(), String> {
    let path = match account.provider {
        // The session tells whether the token may use masked addresses.
        Provider::Fastmail => return fastmail_account(account).map(drop).map_err(Failure::text),
        Provider::ForwardEmail => "/v1/domains",
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
        Provider::Fastmail => {
            let mut new = json!({ "state": "enabled" });
            if let Some(s) = site {
                new["forDomain"] = json!(s);
                new["description"] = json!(s);
            }
            made(&fastmail_set(account, &json!({ "create": { "new": new } }), "created", "new")?, "email")
        }
        Provider::ForwardEmail => {
            let domain = forward_domain(account)?;
            // No name: the service makes a random one.
            let body = site.map_or_else(|| json!({}), |s| json!({ "description": s }));
            let mut v = call(account, "POST", &format!("/v1/domains/{domain}/aliases"), Some(&body))?;
            v["email"] = json!(format!("{}@{domain}", v["name"].as_str().unwrap_or_default()));
            made(&v, "email")
        }
    };
    run().map_err(Failure::text)
}

fn safe(remote: &str) -> Result<(), String> {
    valid_remote(remote).then_some(()).ok_or_else(|| Failure::BadAnswer.text())
}

/// Switches an alias on or off.
pub fn set_enabled(account: &Account, remote: &str, email: &str, on: bool) -> Result<(), String> {
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
        Provider::Fastmail => {
            let state = if on { "enabled" } else { "disabled" };
            fastmail_set(account, &json!({ "update": { remote: { "state": state } } }), "updated", remote).map(drop)
        }
        Provider::ForwardEmail => {
            let (path, name) = forward_path(remote, email)?;
            call(account, "PUT", &path, Some(&json!({ "name": name, "is_enabled": on }))).map(drop)
        }
    };
    run().map_err(Failure::text)
}

/// Deletes an alias; one the service no longer has counts as deleted.
pub fn delete(account: &Account, remote: &str, email: &str) -> Result<(), String> {
    safe(remote)?;
    let path = match account.provider {
        Provider::SimpleLogin => format!("/api/aliases/{remote}"),
        Provider::Addy => format!("/api/v1/aliases/{remote}"),
        Provider::FirefoxRelay => format!("/api/v1/relayaddresses/{remote}/"),
        Provider::ForwardEmail => forward_path(remote, email).map_err(Failure::text)?.0,
        Provider::Fastmail => {
            let change = json!({ "update": { remote: { "state": "deleted" } } });
            return fastmail_set(account, &change, "updated", remote).map(drop).map_err(Failure::text);
        }
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
        set_enabled(&account(Provider::SimpleLogin, &s), "7", "a@b.c", false).unwrap();
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
        set_enabled(&a, "50c9-ab", "q@mine.test", false).unwrap();
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
        set_enabled(&a, "3", "z@mozmail.test", false).unwrap();
        let seen = run.join().unwrap();
        assert!(seen[0].contains("Authorization: Token key-test"));
        assert!(seen[1].starts_with("PATCH /api/v1/relayaddresses/3/ ") && seen[1].contains("false"));
    }

    #[test]
    fn fastmail_masks_through_its_own_server() {
        const SESSION: &str =
            r#"{"apiUrl":"https://else.test/","primaryAccounts":{"https://www.fastmail.com/dev/maskedemail":"u1_a"}}"#;
        let (s, run) = serve(&[
            ("200 OK", SESSION),
            (
                "200 OK",
                r#"{"methodResponses":[["MaskedEmail/set",{"created":{"new":{"id":"masked-1","email":"m@fm.test"}}},"0"]]}"#,
            ),
            ("200 OK", SESSION),
            ("200 OK", r#"{"methodResponses":[["MaskedEmail/set",{"updated":{"masked-1":null}},"0"]]}"#),
            ("200 OK", SESSION),
            (
                "200 OK",
                r#"{"methodResponses":[["MaskedEmail/set",{"notCreated":{"new":{"type":"rateLimit"}}},"0"]]}"#,
            ),
            ("200 OK", r#"{"primaryAccounts":{}}"#),
        ]);
        let a = account(Provider::Fastmail, &s);
        let m = create(&a, None).unwrap();
        assert_eq!((m.remote.as_str(), m.email.as_str()), ("masked-1", "m@fm.test"));
        delete(&a, &m.remote, &m.email).unwrap();
        assert_eq!(create(&a, None), Err(message("msg.alias_refused", &[("why", "rateLimit")])));
        assert_eq!(check(&a), Err(message("msg.alias_key_refused", &[])), "a token without the scope");
        let seen = run.join().unwrap();
        assert!(seen[1].starts_with("POST /jmap/api/ ") && seen[1].contains("u1_a"));
        assert!(!seen[1].contains("forDomain") && seen[3].contains("deleted"));
    }

    #[test]
    fn forward_email_uses_a_domain_of_the_account_and_keeps_the_name() {
        let (s, run) = serve(&[
            ("200 OK", r#"[{"name":"mine.test"}]"#),
            ("200 OK", r#"{"id":"64ab","name":"rand.7","domain":{"name":"mine.test"}}"#),
            ("200 OK", "{}"),
            ("200 OK", "{}"),
        ]);
        let a = account(Provider::ForwardEmail, &s);
        let m = create(&a, None).unwrap();
        assert_eq!(m.email, "rand.7@mine.test");
        set_enabled(&a, &m.remote, &m.email, false).unwrap();
        delete(&a, &m.remote, &m.email).unwrap();
        let seen = run.join().unwrap();
        assert!(seen[0].contains("Authorization: Basic a2V5LXRlc3Q6"), "{}", seen[0]);
        assert!(seen[2].starts_with("PUT /v1/domains/mine.test/aliases/64ab ") && seen[2].contains("rand.7"));
        assert!(seen[3].starts_with("DELETE /v1/domains/mine.test/aliases/64ab "));
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
        assert_eq!(delete(&a, "9", "a@b.c"), Ok(()), "already gone");
        assert!(create(&a, None).is_err(), "not an address");
        assert!(check(&a).is_err(), "not the service");
        assert_eq!(run.join().unwrap().len(), 6);
        assert!(delete(&a, "../x", "a@b.c").is_err());
    }
}
