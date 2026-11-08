//! The password wallet file (`passwords.bin`), kept apart from the older `vault.bin` whose key
//! sits in the system keychain with no secret from the user.
//!
//! The entries are sealed with XChaCha20-Poly1305 under a random 256-bit data key. That key is
//! itself wrapped either by a secret kept in the system keychain (Windows Credential Manager,
//! macOS Keychain: unlocked with the user's session) or by a key derived from a master
//! password with Argon2id (unlocked on demand, locked again after a while). The header (format,
//! device, KDF parameters, wrapped key) is authenticated with the content, so it cannot be
//! swapped. The file is JSON text: it goes through `write_later` like the other user data.

use blackhole_domain::passwords::{generate, Entry, Kind, Passwords, Recipe};
use chacha20poly1305::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng, Payload},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use crate::store::Vault;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

pub const FILE: &str = "passwords.bin";
const FORMAT: u32 = 1;
const WRAP_AAD: &[u8] = b"blackhole-wallet-key-v1";

/// Argon2id cost: 64 MiB, 3 passes (OWASP's second recommended setting).
#[cfg(not(test))]
const KDF_COST: (u32, u32, u32) = (64 * 1024, 3, 1);
#[cfg(test)]
const KDF_COST: (u32, u32, u32) = (256, 1, 1);

/// How the data key is unlocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protection {
    /// A secret in the system keychain: open as long as the user's session is.
    Keychain,
    /// A master password, asked again after the lock delay.
    Password,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The master password is needed (`open(None)` on a protected wallet).
    Locked,
    WrongPassword,
    Other(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Locked => f.write_str("Passwords are locked"),
            Self::WrongPassword => f.write_str("Wrong master password"),
            Self::Other(e) => f.write_str(e),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Kdf {
    m: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Wrap {
    nonce: String,
    key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    format: u32,
    device: String,
    protection: Protection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kdf: Option<Kdf>,
    wrap: Wrap,
}

#[derive(Serialize, Deserialize)]
struct FileDto {
    #[serde(flatten)]
    header: Header,
    nonce: String,
    data: String,
}

#[derive(Serialize, Deserialize)]
struct EntryDto {
    id: String,
    origin: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    notes: String,
    /// `None`: a login form; `Some(realm)`: HTTP authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    realm: Option<String>,
    created: i64,
    modified: i64,
    device: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    deleted: bool,
}

impl Drop for EntryDto {
    fn drop(&mut self) {
        self.password.zeroize();
        self.notes.zeroize();
    }
}

#[derive(Serialize, Deserialize, Default)]
struct BodyDto {
    #[serde(default)]
    entries: Vec<EntryDto>,
    #[serde(default)]
    never: Vec<String>,
}

fn to_dto(e: &Entry) -> EntryDto {
    EntryDto {
        id: e.id.clone(),
        origin: e.origin.clone(),
        username: e.username.clone(),
        password: e.password.clone(),
        notes: e.notes.clone(),
        realm: match &e.kind {
            Kind::Web => None,
            Kind::HttpAuth { realm } => Some(realm.clone()),
        },
        created: e.created,
        modified: e.modified,
        device: e.device.clone(),
        deleted: e.deleted,
    }
}

fn from_dto(d: &mut EntryDto) -> Entry {
    Entry {
        id: std::mem::take(&mut d.id),
        origin: std::mem::take(&mut d.origin),
        username: std::mem::take(&mut d.username),
        password: std::mem::take(&mut d.password),
        notes: std::mem::take(&mut d.notes),
        kind: d
            .realm
            .take()
            .map_or(Kind::Web, |realm| Kind::HttpAuth { realm }),
        created: d.created,
        modified: d.modified,
        device: std::mem::take(&mut d.device),
        deleted: d.deleted,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 == 1 || !s.is_ascii() {
        return Err("fichier de mots de passe corrompu".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

/// A fresh entry id: 128 random bits.
#[must_use]
pub fn new_id() -> String {
    hex(&random::<16>())
}

/// A strong password from the system's random generator.
#[must_use]
pub fn random_password(recipe: Recipe) -> String {
    generate(recipe, || random::<1>()[0])
}

fn kdf(password: &str, k: &Kdf) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = argon2::Params::new(k.m, k.t, k.p, Some(32)).map_err(|e| e.to_string())?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), &unhex(&k.salt)?, out.as_mut())
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn wrap_key(kek: &[u8; 32], key: &[u8; 32]) -> Result<Wrap, String> {
    let nonce = random::<24>();
    let sealed = XChaCha20Poly1305::new(kek.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: key, aad: WRAP_AAD })
        .map_err(|e| e.to_string())?;
    Ok(Wrap {
        nonce: hex(&nonce),
        key: hex(&sealed),
    })
}

fn unwrap_key(kek: &[u8; 32], w: &Wrap) -> Option<Zeroizing<[u8; 32]>> {
    let nonce = unhex(&w.nonce).ok().filter(|n| n.len() == 24)?;
    let plain = Zeroizing::new(
        XChaCha20Poly1305::new(kek.into())
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &unhex(&w.key).ok()?,
                    aad: WRAP_AAD,
                },
            )
            .ok()?,
    );
    let mut key = Zeroizing::new([0u8; 32]);
    if plain.len() != 32 {
        return None;
    }
    key.copy_from_slice(&plain);
    Some(key)
}

/// Where the keychain secret comes from: the system, or a fixed key in tests.
#[derive(Clone)]
enum Keychain {
    Os,
    #[cfg(test)]
    Fixed([u8; 32]),
}

impl Keychain {
    /// The wallet's secret in the system keychain, created on first use. Windows keeps it in
    /// the Credential Manager (DPAPI, per user); macOS in the login Keychain (not run in CI
    /// here: checked by cross-compilation only). Other systems have no keychain support.
    fn secret(&self) -> Result<Zeroizing<[u8; 32]>, String> {
        match self {
            #[cfg(test)]
            Self::Fixed(k) => Ok(Zeroizing::new(*k)),
            Self::Os => os_secret(),
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn os_secret() -> Result<Zeroizing<[u8; 32]>, String> {
    // keyring's `windows-native` / `apple-native` backends: Credential Manager / Keychain.
    let entry = keyring::Entry::new("BlackHole", "passwords-key").map_err(|e| e.to_string())?;
    match entry.get_secret() {
        Ok(mut s) => {
            let mut k = Zeroizing::new([0u8; 32]);
            let ok = s.len() == 32;
            if ok {
                k.copy_from_slice(&s);
            }
            s.zeroize();
            ok.then_some(k)
                .ok_or_else(|| "clé du trousseau invalide".to_string())
        }
        Err(keyring::Error::NoEntry) => {
            let k = Zeroizing::new(random::<32>());
            entry.set_secret(k.as_ref()).map_err(|e| e.to_string())?;
            Ok(k)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_secret() -> Result<Zeroizing<[u8; 32]>, String> {
    Err("no system keychain on this platform: use a master password".into())
}

/// The wallet file of a profile.
#[derive(Clone)]
pub struct Wallet {
    path: PathBuf,
    keychain: Keychain,
}

/// An open wallet: the data key and the decrypted entries, wiped when dropped.
pub struct Unlocked {
    key: Zeroizing<[u8; 32]>,
    header: Header,
    pub data: Passwords,
}

impl Unlocked {
    #[must_use]
    pub fn protection(&self) -> Protection {
        self.header.protection
    }

    /// This device's id, recorded in every change it makes.
    #[must_use]
    pub fn device(&self) -> &str {
        &self.header.device
    }

    /// The file content: entries sealed under the data key (fresh nonce), header authenticated.
    pub fn seal(&self) -> Result<String, String> {
        let body = BodyDto {
            entries: self.data.entries.iter().map(to_dto).collect(),
            never: self.data.never.clone(),
        };
        let plain = Zeroizing::new(serde_json::to_vec(&body).map_err(|e| e.to_string())?);
        let aad = serde_json::to_vec(&self.header).map_err(|e| e.to_string())?;
        let nonce = random::<24>();
        let sealed = XChaCha20Poly1305::new(self.key.as_ref().into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: &aad,
                },
            )
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&FileDto {
            header: self.header.clone(),
            nonce: hex(&nonce),
            data: hex(&sealed),
        })
        .map_err(|e| e.to_string())
    }

    /// From now on the data key is unlocked by `password` (new salt), not the keychain.
    pub fn protect_with_password(&mut self, password: &str) -> Result<(), String> {
        if password.chars().count() < 8 {
            return Err("The master password needs at least 8 characters".into());
        }
        let (m, t, p) = KDF_COST;
        let k = Kdf {
            m,
            t,
            p,
            salt: hex(&random::<16>()),
        };
        let kek = kdf(password, &k)?;
        self.header.wrap = wrap_key(&kek, &self.key)?;
        self.header.kdf = Some(k);
        self.header.protection = Protection::Password;
        Ok(())
    }

    /// Back to the keychain secret of `wallet` (no master password).
    pub fn protect_with_keychain(&mut self, wallet: &Wallet) -> Result<(), String> {
        let kek = wallet.keychain.secret()?;
        self.header.wrap = wrap_key(&kek, &self.key)?;
        self.header.kdf = None;
        self.header.protection = Protection::Keychain;
        Ok(())
    }
}

impl Wallet {
    #[must_use]
    pub fn new(data: &Path) -> Self {
        Self {
            path: data.join(FILE),
            keychain: Keychain::Os,
        }
    }

    #[cfg(test)]
    fn with_keychain_key(data: &Path, key: [u8; 32]) -> Self {
        Self {
            path: data.join(FILE),
            keychain: Keychain::Fixed(key),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<Option<FileDto>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|_| "fichier de mots de passe corrompu".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// How the file is protected; `None` while it does not exist yet.
    pub fn protection(&self) -> Result<Option<Protection>, String> {
        Ok(self.read()?.map(|f| f.header.protection))
    }

    /// A new empty wallet protected by the keychain (written by the caller with `seal`).
    pub fn create(&self) -> Result<Unlocked, String> {
        let key = Zeroizing::new(random::<32>());
        let kek = self.keychain.secret()?;
        let header = Header {
            format: FORMAT,
            device: hex(&random::<8>()),
            protection: Protection::Keychain,
            kdf: None,
            wrap: wrap_key(&kek, &key)?,
        };
        Ok(Unlocked {
            key,
            header,
            data: Passwords::default(),
        })
    }

    /// Opens the wallet: with the keychain, or with `password` when it has a master password.
    pub fn open(&self, password: Option<&str>) -> Result<Unlocked, OpenError> {
        let file = self
            .read()
            .map_err(OpenError::Other)?
            .ok_or_else(|| OpenError::Other("no password file".into()))?;
        if file.header.format > FORMAT {
            return Err(OpenError::Other(
                "passwords saved by a newer BlackHole".into(),
            ));
        }
        let key = match (&file.header.protection, &file.header.kdf, password) {
            (Protection::Keychain, _, _) => {
                let kek = self.keychain.secret().map_err(OpenError::Other)?;
                unwrap_key(&kek, &file.header.wrap)
                    .ok_or_else(|| OpenError::Other("keychain key does not match".into()))?
            }
            (Protection::Password, Some(k), Some(pw)) => {
                let kek = kdf(pw, k).map_err(OpenError::Other)?;
                unwrap_key(&kek, &file.header.wrap).ok_or(OpenError::WrongPassword)?
            }
            (Protection::Password, None, _) => {
                return Err(OpenError::Other("fichier de mots de passe corrompu".into()))
            }
            (Protection::Password, _, None) => return Err(OpenError::Locked),
        };
        let aad = serde_json::to_vec(&file.header).map_err(|e| OpenError::Other(e.to_string()))?;
        let nonce = unhex(&file.nonce)
            .ok()
            .filter(|n| n.len() == 24)
            .ok_or_else(|| OpenError::Other("fichier de mots de passe corrompu".into()))?;
        let sealed = unhex(&file.data).map_err(OpenError::Other)?;
        let plain = Zeroizing::new(
            XChaCha20Poly1305::new(key.as_ref().into())
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &sealed,
                        aad: &aad,
                    },
                )
                .map_err(|_| {
                    OpenError::Other("password file was modified or is damaged".into())
                })?,
        );
        let mut body: BodyDto =
            serde_json::from_slice(&plain).map_err(|e| OpenError::Other(e.to_string()))?;
        let entries = body.entries.iter_mut().map(from_dto).collect();
        Ok(Unlocked {
            key,
            header: file.header,
            data: Passwords {
                entries,
                never: std::mem::take(&mut body.never),
            },
        })
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// At launch (off the UI thread): the wallet opened with the keychain, or `None` while it
/// waits for its master password. The first time it is created with the logins of the older
/// `vault.bin`, which is deleted once the new file reads back the same. A vault that cannot
/// be read is left in place.
pub fn open_at_start(wallet: &Wallet, vault: &Vault, now: i64) -> Result<Option<Unlocked>, String> {
    match wallet.protection()? {
        Some(Protection::Password) => return Ok(None),
        Some(Protection::Keychain) => return wallet.open(None).map(Some).map_err(|e| e.to_string()),
        None => {}
    }
    let mut u = wallet.create()?;
    let device = u.device().to_owned();
    let mut old = if vault.exists() { vault.load().ok() } else { None };
    if let Some(list) = &old {
        let logins: Vec<(&str, &str, &str)> = list
            .iter()
            .map(|l| (l.url.as_str(), l.username.as_str(), l.password.as_str()))
            .collect();
        u.data.import(&logins, new_id, now, &device);
    }
    let had_old = old.is_some();
    for l in old.iter_mut().flatten() {
        l.password.zeroize();
    }
    write_atomic(wallet.path(), &u.seal()?)?;
    let back = wallet.open(None).map_err(|e| e.to_string())?;
    if had_old && back.data == u.data {
        vault.remove()?;
    }
    Ok(Some(back))
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackhole_domain::passwords::Draft;

    fn draft<'a>(origin: &'a str, username: &'a str, password: &'a str) -> Draft<'a> {
        Draft {
            origin,
            username,
            password,
            notes: "note-test",
            kind: Kind::Web,
        }
    }

    fn filled(w: &Wallet) -> Unlocked {
        let mut u = w.create().unwrap();
        let device = u.device().to_owned();
        u.data
            .add(new_id(), &draft("https://example.test", "alice", "pw-test-1"), 1, &device);
        u.data.add(
            new_id(),
            &Draft {
                kind: Kind::HttpAuth { realm: "admin".into() },
                ..draft("https://example.test", "root", "pw-test-2")
            },
            2,
            &device,
        );
        u.data.never_for("https://never.test");
        u
    }

    #[test]
    fn keychain_roundtrip_and_nothing_in_clear() {
        let t = tempfile::tempdir().unwrap();
        let w = Wallet::with_keychain_key(t.path(), [3; 32]);
        assert_eq!(w.protection().unwrap(), None);
        let u = filled(&w);
        let text = u.seal().unwrap();
        for secret in ["pw-test-1", "alice", "note-test", "example.test", "admin"] {
            assert!(!text.contains(secret), "{secret} in clear");
        }
        std::fs::write(w.path(), &text).unwrap();
        assert_eq!(w.protection().unwrap(), Some(Protection::Keychain));
        let back = w.open(None).unwrap();
        assert_eq!(back.data, u.data);
        assert_eq!(back.device(), u.device());
        // Another keychain secret cannot open it.
        let other = Wallet::with_keychain_key(t.path(), [4; 32]);
        assert!(matches!(other.open(None), Err(OpenError::Other(_))));
    }

    #[test]
    fn master_password_roundtrip_and_wrong_password() {
        let t = tempfile::tempdir().unwrap();
        let w = Wallet::with_keychain_key(t.path(), [3; 32]);
        let mut u = filled(&w);
        let pw = random_password(Recipe::default());
        assert!(u.protect_with_password("short").is_err());
        u.protect_with_password(&pw).unwrap();
        std::fs::write(w.path(), u.seal().unwrap()).unwrap();
        assert_eq!(w.protection().unwrap(), Some(Protection::Password));
        assert_eq!(w.open(None).err(), Some(OpenError::Locked));
        assert_eq!(w.open(Some("not-the-password")).err(), Some(OpenError::WrongPassword));
        let mut back = w.open(Some(&pw)).unwrap();
        assert_eq!(back.data, u.data);
        // And back to the keychain, same entries.
        back.protect_with_keychain(&w).unwrap();
        std::fs::write(w.path(), back.seal().unwrap()).unwrap();
        assert_eq!(w.open(None).unwrap().data, u.data);
    }

    #[test]
    fn tampered_header_or_content_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let w = Wallet::with_keychain_key(t.path(), [3; 32]);
        let text = filled(&w).seal().unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["device"] = "0000000000000000".into();
        std::fs::write(w.path(), v.to_string()).unwrap();
        assert!(w.open(None).is_err());
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let data = v["data"].as_str().unwrap().to_owned();
        let flipped = if data.starts_with('0') { "1" } else { "0" };
        v["data"] = format!("{flipped}{}", &data[1..]).into();
        std::fs::write(w.path(), v.to_string()).unwrap();
        assert!(w.open(None).is_err());
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce() {
        let t = tempfile::tempdir().unwrap();
        let w = Wallet::with_keychain_key(t.path(), [3; 32]);
        let u = filled(&w);
        assert_ne!(u.seal().unwrap(), u.seal().unwrap());
        assert_ne!(new_id(), new_id());
        assert_eq!(new_id().len(), 32);
    }

    #[test]
    fn first_launch_moves_the_old_vault_logins() {
        let t = tempfile::tempdir().unwrap();
        let vault = Vault::named_with_key(t.path(), "vault.bin", [9; 32]);
        vault
            .merge(vec![crate::import::Login {
                url: "https://example.test/login".into(),
                username: "alice".into(),
                password: "pw-test-1".into(),
            }])
            .unwrap();
        let w = Wallet::with_keychain_key(t.path(), [3; 32]);
        let u = open_at_start(&w, &vault, 5).unwrap().unwrap();
        assert_eq!(u.data.for_origin("https://example.test")[0].password, "pw-test-1");
        assert!(!vault.exists());
        // Next launches just open it.
        assert_eq!(open_at_start(&w, &vault, 6).unwrap().unwrap().data, u.data);
        // With a master password, it waits for it.
        let mut u = w.open(None).unwrap();
        u.protect_with_password("master-test-pw").unwrap();
        std::fs::write(w.path(), u.seal().unwrap()).unwrap();
        assert!(open_at_start(&w, &vault, 7).unwrap().is_none());
    }
}
