//! Import of bookmarks and logins from other browsers (Chromium and Firefox families).
//!
//! Bookmarks are read straight from the other browser's profile (read-only, on a temporary copy).
//! Logins are only imported from the CSV export the user produced in the source browser: the
//! encrypted stores of other browsers are never touched.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Chromium,
    Firefox,
}

#[derive(Debug, Clone)]
pub struct Source {
    pub id: String,
    pub browser: String,
    pub profile: String,
    pub family: Family,
    pub path: PathBuf,
}

struct Known {
    name: &'static str,
    family: Family,
    /// Relative to the platform's application data directory.
    mac: &'static str,
    win: &'static str,
}

const KNOWN: &[Known] = &[
    Known {
        name: "Chrome",
        family: Family::Chromium,
        mac: "Google/Chrome",
        win: "Google/Chrome/User Data",
    },
    Known {
        name: "Brave",
        family: Family::Chromium,
        mac: "BraveSoftware/Brave-Browser",
        win: "BraveSoftware/Brave-Browser/User Data",
    },
    Known {
        name: "Edge",
        family: Family::Chromium,
        mac: "Microsoft Edge",
        win: "Microsoft/Edge/User Data",
    },
    Known {
        name: "Vivaldi",
        family: Family::Chromium,
        mac: "Vivaldi",
        win: "Vivaldi/User Data",
    },
    Known {
        name: "Opera",
        family: Family::Chromium,
        mac: "com.operasoftware.Opera",
        win: "Opera Software/Opera Stable",
    },
    Known {
        name: "Arc",
        family: Family::Chromium,
        mac: "Arc/User Data",
        win: "Arc/User Data",
    },
    Known {
        name: "Firefox",
        family: Family::Firefox,
        mac: "Firefox/Profiles",
        win: "Mozilla/Firefox/Profiles",
    },
    Known {
        name: "Waterfox",
        family: Family::Firefox,
        mac: "Waterfox/Profiles",
        win: "Waterfox/Profiles",
    },
    Known {
        name: "LibreWolf",
        family: Family::Firefox,
        mac: "LibreWolf/Profiles",
        win: "LibreWolf/Profiles",
    },
    Known {
        name: "Zen",
        family: Family::Firefox,
        mac: "zen/Profiles",
        win: "zen/Profiles",
    },
];

/// Browsers with importable data found under `base` (the platform application-data directory).
pub fn detect_in(base: &Path, windows: bool) -> Vec<Source> {
    let mut out = Vec::new();
    for k in KNOWN {
        let root = base.join(if windows { k.win } else { k.mac });
        match k.family {
            Family::Chromium => {
                let mut dirs = vec![root.clone()];
                if let Ok(rd) = fs::read_dir(&root) {
                    let mut subs: Vec<_> = rd
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect();
                    subs.sort();
                    dirs.extend(subs);
                }
                for d in dirs.into_iter().filter(|d| d.join("Bookmarks").is_file()) {
                    out.push(source(k, &d));
                }
            }
            Family::Firefox => {
                let Ok(rd) = fs::read_dir(&root) else {
                    continue;
                };
                let mut subs: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                subs.sort();
                for d in subs
                    .into_iter()
                    .filter(|d| d.join("places.sqlite").is_file())
                {
                    out.push(source(k, &d));
                }
            }
        }
    }
    out
}

fn source(k: &Known, dir: &Path) -> Source {
    let profile = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let profile = profile
        .split_once('.')
        .filter(|_| k.family == Family::Firefox)
        .map_or(profile.clone(), |(_, n)| n.to_owned());
    Source {
        id: format!("{}:{}", k.name.to_lowercase(), dir.display()),
        browser: k.name.to_owned(),
        profile,
        family: k.family,
        path: dir.to_path_buf(),
    }
}

pub fn detect() -> Vec<Source> {
    #[cfg(target_os = "windows")]
    let (base, windows) = (dirs::data_local_dir().zip(dirs::data_dir()), true);
    #[cfg(not(target_os = "windows"))]
    let (base, windows) = (dirs::data_dir().map(|d| (d.clone(), d)), false);
    let Some((local, roaming)) = base else {
        return Vec::new();
    };
    let mut all = detect_in(&local, windows);
    if windows {
        // Opera and Firefox live under Roaming on Windows.
        let extra: Vec<_> = detect_in(&roaming, true)
            .into_iter()
            .filter(|s| !all.iter().any(|o| o.path == s.path))
            .collect();
        all.extend(extra);
    }
    all
}

pub fn read_bookmarks(source: &Source) -> Result<Vec<Bookmark>, String> {
    match source.family {
        Family::Chromium => {
            let file = source.path.join("Bookmarks");
            let raw = fs::read_to_string(&file).map_err(|e| format!("{} : {e}", file.display()))?;
            parse_chromium(&raw)
        }
        Family::Firefox => read_firefox(&source.path),
    }
}

fn importable(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

pub fn parse_chromium(json: &str) -> Result<Vec<Bookmark>, String> {
    fn walk(node: &serde_json::Value, folder: &str, out: &mut Vec<Bookmark>) {
        let name = node["name"].as_str().unwrap_or_default();
        match node["type"].as_str() {
            Some("url") => {
                if let Some(url) = node["url"].as_str().filter(|u| importable(u)) {
                    out.push(Bookmark {
                        title: name.to_owned(),
                        url: url.to_owned(),
                        folder: folder.to_owned(),
                    });
                }
            }
            Some("folder") => {
                let path = if folder.is_empty() {
                    name.to_owned()
                } else {
                    format!("{folder}/{name}")
                };
                for c in node["children"].as_array().into_iter().flatten() {
                    walk(c, &path, out);
                }
            }
            _ => {}
        }
    }
    let root: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for key in ["bookmark_bar", "other", "synced"] {
        for c in root["roots"][key]["children"]
            .as_array()
            .into_iter()
            .flatten()
        {
            walk(c, "", &mut out);
        }
    }
    Ok(out)
}

fn read_firefox(dir: &Path) -> Result<Vec<Bookmark>, String> {
    // The live database is locked and may sit in a WAL: work on a copy.
    let tmp = tempfile_dir()?;
    for name in ["places.sqlite", "places.sqlite-wal"] {
        let from = dir.join(name);
        if from.is_file() {
            fs::copy(&from, tmp.join(name)).map_err(|e| e.to_string())?;
        }
    }
    let db = rusqlite::Connection::open_with_flags(
        tmp.join("places.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT COALESCE(b.title, p.title, ''), p.url, COALESCE(f.title, '')
             FROM moz_bookmarks b
             JOIN moz_places p ON p.id = b.fk
             LEFT JOIN moz_bookmarks f ON f.id = b.parent
             WHERE b.type = 1 ORDER BY b.id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Bookmark {
                title: r.get(0)?,
                url: r.get(1)?,
                folder: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let out = rows.flatten().filter(|b| importable(&b.url)).collect();
    drop(stmt);
    drop(db);
    let _ = fs::remove_dir_all(&tmp);
    Ok(out)
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let mut n = [0u8; 8];
    chacha20poly1305::aead::rand_core::RngCore::fill_bytes(
        &mut chacha20poly1305::aead::OsRng,
        &mut n,
    );
    let dir = std::env::temp_dir().join(format!(
        "blackhole-import-{}",
        n.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Bookmarks exported as a Netscape HTML file (every browser can produce one).
pub fn parse_html(html: &str) -> Vec<Bookmark> {
    let mut out = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let start = i + off;
        let Some(end) = lower[start..].find('>').map(|e| start + e) else {
            break;
        };
        let tag = &lower[start + 1..end];
        let text_end = lower[end..].find('<').map_or(html.len(), |e| end + e);
        let text = html_unescape(html[end + 1..text_end].trim());
        if tag.starts_with("h3") {
            folders.push(text);
        } else if tag.starts_with("/dl") {
            folders.pop();
        } else if tag.starts_with("a ") {
            let attrs = &html[start + 1..end];
            if let Some(url) = attr(attrs, "href").filter(|u| importable(u)) {
                out.push(Bookmark {
                    title: text,
                    url,
                    folder: folders.join("/"),
                });
            }
        }
        i = end + 1;
    }
    out
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let pos = lower.find(&format!("{name}=\""))? + name.len() + 2;
    let len = attrs[pos..].find('"')?;
    Some(html_unescape(&attrs[pos..pos + len]))
}

fn html_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Logins from a CSV export (Chrome, Edge, Brave, Firefox, Safari, Bitwarden headers are recognised).
pub fn parse_logins_csv(data: &str) -> Result<Vec<Login>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(data.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let col = |names: &[&str]| headers.iter().position(|h| names.contains(&h.as_str()));
    let (Some(u), Some(n), Some(p)) = (
        col(&["url", "login_uri", "website", "origin"]),
        col(&["username", "login_username", "user"]),
        col(&["password", "login_password"]),
    ) else {
        return Err("Colonnes url / username / password introuvables".into());
    };
    let mut out = Vec::new();
    for rec in rdr.records().flatten() {
        let (url, username, password) = (
            rec.get(u).unwrap_or(""),
            rec.get(n).unwrap_or(""),
            rec.get(p).unwrap_or(""),
        );
        if importable(url) && !password.is_empty() {
            out.push(Login {
                url: url.to_owned(),
                username: username.to_owned(),
                password: password.to_owned(),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_bookmarks_keep_folder_paths_and_skip_non_web_urls() {
        let json = r#"{"roots":{"bookmark_bar":{"children":[
            {"type":"url","name":"Rust","url":"https://rust-lang.org/"},
            {"type":"folder","name":"Dev","children":[
                {"type":"url","name":"Docs","url":"https://doc.rust-lang.org/"},
                {"type":"url","name":"JS","url":"javascript:alert(1)"}]}]},
            "other":{"children":[]}}}"#;
        let b = parse_chromium(json).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(
            b[1],
            Bookmark {
                title: "Docs".into(),
                url: "https://doc.rust-lang.org/".into(),
                folder: "Dev".into()
            }
        );
    }

    #[test]
    fn netscape_html_export_is_parsed() {
        let html = r#"<DL><p><DT><H3>Work</H3><DL><p><DT><A HREF="https://a.com/?x=1&amp;y=2" ADD_DATE="1">A &amp; B</A></DL><p><DT><A HREF="https://b.com/">B</A></DL>"#;
        let b = parse_html(html);
        assert_eq!(
            b[0],
            Bookmark {
                title: "A & B".into(),
                url: "https://a.com/?x=1&y=2".into(),
                folder: "Work".into()
            }
        );
        assert_eq!(b[1].folder, "");
    }

    #[test]
    fn logins_csv_handles_chrome_firefox_and_bitwarden_headers() {
        let chrome = "name,url,username,password,note\nGH,https://github.com/login,me,\"p,w\",\n";
        assert_eq!(parse_logins_csv(chrome).unwrap()[0].password, "p,w");
        let ff =
            "\"url\",\"username\",\"password\",\"httpRealm\"\n\"https://x.org\",\"u\",\"s\",\"\"\n";
        assert_eq!(parse_logins_csv(ff).unwrap()[0].username, "u");
        let bw = "folder,login_uri,login_username,login_password\n,https://y.org,a,b\n";
        assert_eq!(parse_logins_csv(bw).unwrap().len(), 1);
        assert!(parse_logins_csv("a,b\n1,2\n").is_err());
    }

    #[test]
    fn detects_profiles_of_both_families() {
        let t = tempfile::tempdir().unwrap();
        let chrome = t.path().join("Google/Chrome/Profile 1");
        let ff = t.path().join("Waterfox/Profiles/abc.default");
        fs::create_dir_all(&chrome).unwrap();
        fs::create_dir_all(&ff).unwrap();
        fs::write(chrome.join("Bookmarks"), "{}").unwrap();
        fs::write(ff.join("places.sqlite"), "").unwrap();
        let found = detect_in(t.path(), false);
        let names: Vec<_> = found
            .iter()
            .map(|s| (s.browser.as_str(), s.profile.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![("Chrome", "Profile 1"), ("Waterfox", "default")]
        );
    }

    #[test]
    fn firefox_places_database_is_read_without_touching_the_original() {
        let t = tempfile::tempdir().unwrap();
        let db = rusqlite::Connection::open(t.path().join("places.sqlite")).unwrap();
        db.execute_batch(
            "CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url TEXT, title TEXT);
             CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, parent INTEGER, title TEXT);
             INSERT INTO moz_places VALUES (1,'https://mozilla.org/','Mozilla'),(2,'place:sort=8','Query');
             INSERT INTO moz_bookmarks VALUES (1,2,NULL,0,'Toolbar'),(2,1,1,1,'Moz'),(3,1,2,1,'Q');",
        )
        .unwrap();
        drop(db);
        let b = read_firefox(t.path()).unwrap();
        assert_eq!(
            b,
            vec![Bookmark {
                title: "Moz".into(),
                url: "https://mozilla.org/".into(),
                folder: "Toolbar".into()
            }]
        );
    }
}
