//! `bh-market`: keeps the extension market (`market/market.json`).
//!
//! ```text
//! bh-market keygen <secret-file>          new Ed25519 key: secret (hex) to the file, public key printed
//! bh-market check  [file]                 the list follows every rule the app applies
//! bh-market audit  [file] [--write]       downloads each pinned package, checks its SHA-256,
//!                                         reads its manifest (size, permissions)
//! bh-market update [file] [--summary md]  follows the upstream releases (`source` of each entry):
//!                                         new version → new URLs, hashes, sizes, permissions
//! bh-market review <base> <head>          exit 0 when `head` only brings newer versions asking
//!                                         for no new permission (merged by the workflow after 48 h)
//! bh-market sign   [file] <out-dir> [--days N]
//!                                         stamps `seq` (now) and `expires`, writes market.json
//!                                         and market.json.sig (key: MARKET_SIGNING_KEY, hex)
//! bh-market verify <market.json> <market.json.sig>
//! ```
//!
//! `update` reads the GitHub REST API (`GITHUB_TOKEN` when set, as in the workflow): it runs in
//! CI, never in the app, which only downloads the signed result.

use blackhole_domain::market as rules;
use blackhole_infrastructure::market::{self, Entry, Index, Package};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

const DEFAULT_FILE: &str = "market/market.json";
const MAX_PACKAGE: u64 = 64 * 1024 * 1024;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") => keygen(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("audit") => audit(&args[1..]),
        Some("update") => update(&args[1..]),
        Some("sign") => sign(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("review") => review(&args[1..]),
        _ => Err("usage: bh-market keygen|check|audit|update|review|sign|verify (see the source)".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("bh-market: {why}");
            ExitCode::FAILURE
        }
    }
}

/// Positional arguments (flags and their values left out).
fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a == "--summary" || a == "--days" {
            skip = true;
        } else if !a.starts_with("--") {
            out.push(a.as_str());
        }
    }
    out
}

fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn load(path: &Path) -> Result<Index, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    market::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn save(path: &Path, index: &Index) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    json.push('\n');
    fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
}

fn keygen(args: &[String]) -> Result<(), String> {
    let out = positional(args)
        .first()
        .map(PathBuf::from)
        .ok_or("keygen <secret-file>")?;
    if out.exists() {
        return Err(format!("{} exists: not overwritten", out.display()));
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(&seed);
    fs::write(&out, market::hex(&seed)).map_err(|e| e.to_string())?;
    println!("{}", market::hex(key.verifying_key().as_bytes()));
    Ok(())
}

fn file_arg(args: &[String]) -> PathBuf {
    PathBuf::from(positional(args).first().copied().unwrap_or(DEFAULT_FILE))
}

fn check(args: &[String]) -> Result<(), String> {
    let index = load(&file_arg(args))?;
    for e in &index.extensions {
        if e.source.is_none() {
            return Err(format!("{}: no source, the release tracker cannot follow it", e.id));
        }
    }
    println!("{} extensions, valid", index.extensions.len());
    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent("blackhole-market-tool")
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(120))
        .build()
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    if !rules::trusted_package_url(url) {
        return Err(format!("untrusted URL {url}"));
    }
    let resp = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(MAX_PACKAGE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_PACKAGE {
        return Err(format!("{url}: package too large"));
    }
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    market::hex(&Sha256::digest(bytes))
}

/// Version and permissions (`permissions` + `host_permissions`) of a WebExtension package.
fn manifest_info(zip: &[u8]) -> Result<(String, Vec<String>), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| e.to_string())?;
    // The manifest at the root, or the shallowest one when the package has a top folder.
    let name = archive
        .file_names()
        .filter(|n| *n == "manifest.json" || n.ends_with("/manifest.json"))
        .min_by_key(|n| n.matches('/').count())
        .map(str::to_owned)
        .ok_or("no manifest.json in the package")?;
    let mut text = String::new();
    archive
        .by_name(&name)
        .map_err(|e| e.to_string())?
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let manifest: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    let version = manifest["version"]
        .as_str()
        .ok_or("manifest without version")?
        .to_owned();
    let mut permissions = BTreeSet::new();
    for key in ["permissions", "host_permissions"] {
        for p in manifest[key].as_array().into_iter().flatten() {
            if let Some(p) = p.as_str() {
                permissions.insert(p.to_owned());
            }
        }
    }
    Ok((version, permissions.into_iter().collect()))
}

/// What the packages of an entry say, every platform together.
struct Audited {
    version: String,
    permissions: Vec<String>,
}

/// Downloads every package of `entry`, checks it against its pinned hash and fills its size;
/// `digests`: GitHub's own `sha256:` digest per platform, when known, must agree too.
fn audit_entry(
    agent: &ureq::Agent,
    entry: &mut Entry,
    digests: &[(String, String)],
) -> Result<Audited, String> {
    let mut version: Option<String> = None;
    let mut permissions = BTreeSet::new();
    for (platform, package) in entry.platforms.iter_mut() {
        let bytes = download(agent, &package.url)?;
        let got = sha256(&bytes);
        if !got.eq_ignore_ascii_case(&package.sha256) {
            return Err(format!(
                "{} ({platform}): SHA-256 {got}, pinned {}",
                entry.id, package.sha256
            ));
        }
        if let Some((_, digest)) = digests.iter().find(|(p, _)| p == platform) {
            if !digest.eq_ignore_ascii_case(&got) {
                return Err(format!("{} ({platform}): GitHub says {digest}, got {got}", entry.id));
            }
        }
        package.size = bytes.len() as u64;
        let (v, perms) = manifest_info(&bytes).map_err(|e| format!("{} ({platform}): {e}", entry.id))?;
        if let Some(first) = &version {
            if *first != v {
                eprintln!("warning: {}: {platform} package is version {v}, not {first}", entry.id);
            }
        } else {
            version = Some(v);
        }
        permissions.extend(perms);
    }
    Ok(Audited {
        version: version.ok_or("no package")?,
        permissions: permissions.into_iter().collect(),
    })
}

fn audit(args: &[String]) -> Result<(), String> {
    let path = file_arg(args);
    let mut index = load(&path)?;
    let agent = agent();
    for entry in &mut index.extensions {
        let found = audit_entry(&agent, entry, &[])?;
        if rules::compare_versions(&found.version, &entry.version)
            != Some(std::cmp::Ordering::Equal)
        {
            eprintln!(
                "warning: {}: manifest says {}, the list {}",
                entry.id, found.version, entry.version
            );
        }
        entry.permissions = found.permissions;
        println!("{} {}: ok", entry.id, entry.version);
    }
    if args.iter().any(|a| a == "--write") {
        save(&path, &index)?;
    }
    Ok(())
}

/// The newest published release whose tag starts with `prefix` (Bitwarden's `browser-v` among
/// its desktop and CLI releases). `releases` is the REST answer, newest first.
fn pick_release<'a>(releases: &'a [Value], prefix: &str) -> Option<&'a Value> {
    releases.iter().find(|r| {
        !r["draft"].as_bool().unwrap_or(true)
            && !r["prerelease"].as_bool().unwrap_or(true)
            && r["tag_name"].as_str().is_some_and(|t| t.starts_with(prefix))
    })
}

/// The release asset matching `pattern`: (download URL, GitHub's SHA-256 digest if published).
fn pick_asset(release: &Value, pattern: &str) -> Option<(String, Option<String>)> {
    release["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        if !rules::asset_matches(pattern, name) {
            return None;
        }
        let url = a["browser_download_url"].as_str()?.to_owned();
        let digest = a["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(str::to_ascii_lowercase);
        Some((url, digest))
    })
}

fn releases(agent: &ureq::Agent, repo: &str) -> Result<Vec<Value>, String> {
    let mut req = agent
        .get(&format!("https://api.github.com/repos/{repo}/releases?per_page=40"))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
    }
    let list: Value = req
        .call()
        .map_err(|e| format!("{repo}: {e}"))?
        .into_json()
        .map_err(|e| format!("{repo}: {e}"))?;
    list.as_array()
        .cloned()
        .ok_or_else(|| format!("{repo}: unexpected answer"))
}

/// One line of the pull request text per entry that changed.
fn describe(old: &Entry, new: &Entry) -> String {
    let mut line = format!("- **{}**: {} → {}", new.name, old.version, new.version);
    let added = rules::added_permissions(&old.permissions, &new.permissions);
    if !added.is_empty() {
        line.push_str(&format!(
            "\n  - ⚠️ new permissions: `{}`",
            added.join("`, `")
        ));
    }
    for (platform, p) in &new.platforms {
        line.push_str(&format!("\n  - {platform}: {} `{}`", p.url, p.sha256));
    }
    line
}

/// Why a proposed list (`head`) needs a human before it reaches users, compared with the list
/// in use (`base`). Empty: only new upstream versions of the same extensions, from the same
/// sources, asking for no permission they did not have; the workflow may merge it by itself.
fn review_reasons(base: &Index, head: &Index) -> Vec<String> {
    let mut reasons = Vec::new();
    for old in &base.extensions {
        if head.find(&old.id).is_none() {
            reasons.push(format!("{}: removed from the list", old.id));
        }
    }
    for new in &head.extensions {
        let Some(old) = base.find(&new.id) else {
            reasons.push(format!("{}: new extension", new.id));
            continue;
        };
        let added = rules::added_permissions(&old.permissions, &new.permissions);
        if !added.is_empty() {
            reasons.push(format!("{}: new permissions `{}`", new.id, added.join("`, `")));
        }
        if new.name != old.name
            || new.description != old.description
            || new.homepage != old.homepage
            || new.category != old.category
            || new.source != old.source
        {
            reasons.push(format!("{}: name, description, homepage, category or source changed", new.id));
        }
        if new.platforms.keys().ne(old.platforms.keys()) {
            reasons.push(format!("{}: platforms changed", new.id));
        }
        if new.platforms != old.platforms && !rules::is_upgrade(&new.version, Some(&old.version)) {
            reasons.push(format!("{}: new packages without a newer version", new.id));
        }
        // A package from another repository than the one the entry follows.
        if let Some(source) = &new.source {
            let prefix = format!("https://github.com/{}/releases/download/", source.repo);
            if new.platforms.values().any(|p| !p.url.starts_with(&prefix)) {
                reasons.push(format!("{}: package outside {}", new.id, source.repo));
            }
        }
    }
    if head.schema != base.schema {
        reasons.push("schema changed".into());
    }
    reasons
}

/// `review <base.json> <head.json>`: exit 0 when the proposed list may be merged without a
/// review, 1 (with the reasons) otherwise.
fn review(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let [base, head] = pos.as_slice() else {
        return Err("review <base.json> <head.json>".into());
    };
    let reasons = review_reasons(&load(Path::new(base))?, &load(Path::new(head))?);
    if reasons.is_empty() {
        println!("no review needed: newer versions only, no new permission");
        Ok(())
    } else {
        Err(format!("review needed:\n- {}", reasons.join("\n- ")))
    }
}

fn update(args: &[String]) -> Result<(), String> {
    let path = file_arg(args);
    let mut index = load(&path)?;
    let base = index.clone();
    let agent = agent();
    let mut changes = Vec::new();
    let mut failures = Vec::new();
    for entry in &mut index.extensions {
        let Some(source) = entry.source.clone() else {
            continue;
        };
        let result = (|| -> Result<Option<String>, String> {
            let list = releases(&agent, &source.repo)?;
            let release = pick_release(&list, &source.tag_prefix)
                .ok_or_else(|| format!("{}: no release tagged {}*", entry.id, source.tag_prefix))?;
            let mut candidate = entry.clone();
            let mut digests = Vec::new();
            for (platform, pattern) in &source.assets {
                let (url, digest) = pick_asset(release, pattern)
                    .ok_or_else(|| format!("{}: no asset {pattern}", entry.id))?;
                if candidate.platforms.get(platform).is_some_and(|p| p.url == url) {
                    continue;
                }
                // The hash is the downloaded file's; GitHub's digest, when published, must agree.
                let bytes = download(&agent, &url)?;
                let sha = sha256(&bytes);
                if let Some(d) = &digest {
                    digests.push((platform.clone(), d.clone()));
                }
                candidate.platforms.insert(
                    platform.clone(),
                    Package { url, sha256: sha, size: bytes.len() as u64 },
                );
            }
            if candidate.platforms == entry.platforms {
                return Ok(None);
            }
            let found = audit_entry(&agent, &mut candidate, &digests)?;
            if !rules::is_upgrade(&found.version, Some(&entry.version)) {
                return Err(format!(
                    "{}: upstream {} is not newer than {}",
                    entry.id, found.version, entry.version
                ));
            }
            candidate.version = found.version;
            candidate.permissions = found.permissions;
            let line = describe(entry, &candidate);
            *entry = candidate;
            Ok(Some(line))
        })();
        match result {
            Ok(Some(line)) => changes.push(line),
            Ok(None) => println!("{}: up to date ({})", entry.id, entry.version),
            Err(why) => failures.push(why),
        }
    }
    index.validate()?;
    if !changes.is_empty() {
        save(&path, &index)?;
    }
    if let Some(summary) = flag_value(args, "--summary") {
        let mut text = String::new();
        if !changes.is_empty() {
            text.push_str("New upstream releases, downloaded and hashed by the market workflow. Once merged, the list is signed and every BlackHole offers these versions.\n\n");
            let reasons = review_reasons(&base, &index);
            if reasons.is_empty() {
                text.push_str("✅ No new permission: the workflow merges this pull request by itself 48 hours after its last change. Close it to stop that.\n\n");
            } else {
                text.push_str("⚠️ **Needs a review** (never merged automatically):\n\n");
                for r in &reasons {
                    text.push_str(&format!("- {r}\n"));
                }
                text.push('\n');
            }
            text.push_str(&changes.join("\n"));
            text.push('\n');
        }
        if !failures.is_empty() {
            text.push_str("\nNot followed this time:\n\n");
            for f in &failures {
                text.push_str(&format!("- {f}\n"));
            }
        }
        fs::write(summary, text).map_err(|e| e.to_string())?;
    }
    for f in &failures {
        eprintln!("warning: {f}");
    }
    println!("{} updated, {} not followed", changes.len(), failures.len());
    Ok(())
}

fn signing_key() -> Result<SigningKey, String> {
    let hex = std::env::var("MARKET_SIGNING_KEY").map_err(|_| "MARKET_SIGNING_KEY is not set")?;
    let seed = market::unhex(&hex)
        .and_then(|b| <[u8; 32]>::try_from(b).ok())
        .ok_or("MARKET_SIGNING_KEY is not a 32-byte hex seed")?;
    Ok(SigningKey::from_bytes(&seed))
}

/// The published bytes: the reviewed list stamped with its sequence and expiry.
fn stamped(index: &Index, now: u64, days: u64) -> Result<Vec<u8>, String> {
    let mut index = index.clone();
    index.seq = now;
    index.expires = now + days * 86_400;
    let mut json = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    json.push(b'\n');
    Ok(json)
}

fn sign(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let (file, out) = match pos.as_slice() {
        [out] => (DEFAULT_FILE, *out),
        [file, out] => (*file, *out),
        _ => return Err("sign [file] <out-dir> [--days N]".into()),
    };
    let days: u64 = flag_value(args, "--days")
        .unwrap_or("30")
        .parse()
        .map_err(|_| "--days N")?;
    let index = load(Path::new(file))?;
    let key = signing_key()?;
    let public = market::hex(key.verifying_key().as_bytes());
    if !market::PUBLIC_KEYS.contains(&public.as_str()) {
        return Err(format!("key {public} is not one of the app's PUBLIC_KEYS"));
    }
    let json = stamped(&index, market::now(), days)?;
    let sig = market::hex(&key.sign(&json).to_bytes());
    // What the app will do with these bytes, before they leave.
    market::decode(&json, &sig, market::PUBLIC_KEYS)?;
    let out = Path::new(out);
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    fs::write(out.join(market::INDEX_FILE), &json).map_err(|e| e.to_string())?;
    fs::write(out.join(market::SIG_FILE), format!("{sig}\n")).map_err(|e| e.to_string())?;
    println!("signed {} extensions, valid {days} days", index.extensions.len());
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let [json, sig] = pos.as_slice() else {
        return Err("verify <market.json> <market.json.sig>".into());
    };
    let bytes = fs::read(json).map_err(|e| e.to_string())?;
    let sig = fs::read_to_string(sig).map_err(|e| e.to_string())?;
    let index = market::decode(&bytes, &sig, market::PUBLIC_KEYS)?;
    rules::accept(index.schema, index.seq, index.expires, market::now(), None)
        .map_err(|why| format!("{why:?}"))?;
    println!("valid: {} extensions, sequence {}", index.extensions.len(), index.seq);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn zip_with(name: &str, manifest: &str) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        w.start_file("background.js", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"1").unwrap();
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        w.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn reads_version_and_permissions() {
        let zip = zip_with(
            "manifest.json",
            r#"{"version":"4.9.135","permissions":["storage","tabs"],"host_permissions":["<all_urls>"]}"#,
        );
        let (v, p) = manifest_info(&zip).unwrap();
        assert_eq!(v, "4.9.135");
        assert_eq!(p, ["<all_urls>", "storage", "tabs"]);
        let nested = zip_with("ext/manifest.json", "\u{feff}{\"version\":\"1\"}");
        assert_eq!(manifest_info(&nested).unwrap(), ("1".into(), vec![]));
    }

    #[test]
    fn picks_the_release_of_the_product() {
        let list = vec![
            json!({"tag_name": "desktop-v2026.11.0", "draft": false, "prerelease": false}),
            json!({"tag_name": "browser-v2026.11.1", "draft": false, "prerelease": true}),
            json!({"tag_name": "browser-v2026.11.0", "draft": false, "prerelease": false,
                   "assets": [
                       {"name": "dist-chrome-2026.11.0.zip", "digest": "sha256:ABCD",
                        "browser_download_url": "https://github.com/bitwarden/clients/releases/download/browser-v2026.11.0/dist-chrome-2026.11.0.zip"},
                       {"name": "dist-firefox-2026.11.0.zip",
                        "browser_download_url": "https://github.com/bitwarden/clients/releases/download/browser-v2026.11.0/dist-firefox-2026.11.0.zip"}
                   ]}),
        ];
        let r = pick_release(&list, "browser-v").unwrap();
        assert_eq!(r["tag_name"], "browser-v2026.11.0");
        let (url, digest) = pick_asset(r, "dist-chrome-*.zip").unwrap();
        assert!(url.ends_with("dist-chrome-2026.11.0.zip"));
        assert_eq!(digest.as_deref(), Some("abcd"));
        assert_eq!(pick_asset(r, "dist-firefox-*.zip").unwrap().1, None);
        assert!(pick_asset(r, "dist-safari-*.zip").is_none());
        assert!(pick_release(&list, "cli-v").is_none());
    }

    #[test]
    fn stamped_index_verifies_and_expires() {
        let key = SigningKey::from_bytes(&[3; 32]);
        let public = market::hex(key.verifying_key().as_bytes());
        let json = stamped(market::builtin(), 1_000, 30).unwrap();
        let sig = market::hex(&key.sign(&json).to_bytes());
        let index = market::decode(&json, &sig, &[&public]).unwrap();
        assert_eq!((index.seq, index.expires), (1_000, 1_000 + 30 * 86_400));
        assert_eq!(index.extensions, market::builtin().extensions);
    }

    #[test]
    fn describes_new_permissions() {
        let old = market::builtin().find("dark-reader").unwrap().clone();
        let mut new = old.clone();
        new.version = "4.9.135".into();
        new.permissions.push("debugger".into());
        let text = describe(&old, &new);
        assert!(text.contains("4.9.134 → 4.9.135"));
        assert!(text.contains("new permissions: `debugger`"));
    }

    /// Dark Reader 4.9.134 → 4.9.135 from its own repository, same permissions.
    fn bumped() -> Index {
        let mut head = market::builtin().clone();
        let e = head.extensions.iter_mut().find(|e| e.id == "dark-reader").unwrap();
        e.version = "4.9.135".into();
        for p in e.platforms.values_mut() {
            p.url = p.url.replace("v4.9.134", "v4.9.135");
            p.sha256 = "1".repeat(64);
        }
        head
    }

    fn reasons_for(edit: impl FnOnce(&mut Entry)) -> Vec<String> {
        let mut head = bumped();
        edit(head.extensions.iter_mut().find(|e| e.id == "dark-reader").unwrap());
        review_reasons(market::builtin(), &head)
    }

    #[test]
    fn quiet_version_bumps_need_no_review() {
        let base = market::builtin();
        assert!(review_reasons(base, base).is_empty());
        assert!(review_reasons(base, &bumped()).is_empty());
        // Fewer permissions is fine too.
        assert!(reasons_for(|e| { e.permissions.pop(); }).is_empty());
    }

    #[test]
    fn anything_else_needs_a_review() {
        assert!(reasons_for(|e| e.permissions.push("debugger".into()))[0].contains("debugger"));
        assert!(!reasons_for(|e| e.version = "4.9.134".into()).is_empty());
        assert!(!reasons_for(|e| e.version = "4.9.0".into()).is_empty());
        assert!(!reasons_for(|e| e.source.as_mut().unwrap().repo = "evil/darkreader".into()).is_empty());
        assert!(!reasons_for(|e| e.name = "Dark Reader Pro".into()).is_empty());
        assert!(!reasons_for(|e| {
            e.platforms.get_mut("windows").unwrap().url =
                "https://github.com/evil/darkreader/releases/download/v4.9.135/x.zip".into();
        })
        .is_empty());
        assert!(!reasons_for(|e| { e.platforms.remove("macos"); }).is_empty());
        let mut added = bumped();
        let mut extra = added.extensions[0].clone();
        extra.id = "new-one".into();
        added.extensions.push(extra);
        assert!(review_reasons(market::builtin(), &added)[0].contains("new extension"));
        let mut removed = bumped();
        removed.extensions.pop();
        assert!(review_reasons(market::builtin(), &removed)[0].contains("removed"));
    }

    #[test]
    fn flags_are_not_positional() {
        let args: Vec<String> = ["a.json", "--summary", "s.md", "--write", "out"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(positional(&args), ["a.json", "out"]);
        assert_eq!(flag_value(&args, "--summary"), Some("s.md"));
    }
}
