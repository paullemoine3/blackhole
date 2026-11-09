//! JNI entry points of `app.blackhole.browser.Native` (Android only).
//!
//! `#[no_mangle]` is what the `unsafe_code` lint reports: it is allowed in this module only.
//! No `unsafe` block is needed: the `jni` crate wraps the raw calls.
#![allow(unsafe_code)]

use crate::core::{Blocker, Core};
use crate::devices::Network;
use zeroize::{Zeroize, Zeroizing};
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// The core lives on the UI thread; the lock only guards against a misbehaving caller.
static CORE: Mutex<Option<Core>> = Mutex::new(None);
/// Read by WebView network threads, without the core's lock.
static BLOCKER: OnceLock<Arc<Blocker>> = OnceLock::new();

fn text(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(String::from).unwrap_or_default()
}

fn reply(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|j| j.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn parse_key(hex: &str) -> Option<Zeroizing<[u8; 32]>> {
    if hex.len() != 64 {
        return None;
    }
    let mut key = Zeroizing::new([0u8; 32]);
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(key)
}

fn with_core(f: impl FnOnce(&mut Core) -> String) -> String {
    let mut guard = CORE.lock().unwrap_or_else(PoisonError::into_inner);
    guard.as_mut().map_or_else(|| "[]".to_owned(), f)
}

/// Opens the profile in `dataDir` (the app's files directory) and starts the paired devices
/// service: `vaultKey` (64 hex digits, kept by the Android Keystore) seals the pairing keys,
/// `deviceName` is what the other devices show, `peer` (debug builds, emulator tests) is an
/// `ip:port` to talk to instead of the local network. Returns the first effects.
#[no_mangle]
pub extern "system" fn Java_app_blackhole_browser_Native_init(
    mut env: JNIEnv,
    _class: JClass,
    data_dir: JString,
    vault_key: JString,
    device_name: JString,
    peer: JString,
) -> jstring {
    let dir = text(&mut env, &data_dir);
    let mut key_hex = Zeroizing::new(text(&mut env, &vault_key));
    let name = text(&mut env, &device_name);
    let peer = text(&mut env, &peer);
    let mut guard = CORE.lock().unwrap_or_else(PoisonError::into_inner);
    // An activity recreated by the system keeps the process: the core stays as it was.
    let core = guard.get_or_insert_with(|| Core::start(std::path::Path::new(&dir)));
    if let Some(key) = parse_key(&key_hex) {
        let network = peer
            .parse::<std::net::SocketAddr>()
            .ok()
            .map_or(Network::Lan, |peer| Network::Direct {
                bind: std::net::SocketAddr::from(([0, 0, 0, 0], blackhole_infrastructure::handoff::PORT)),
                peer,
            });
        core.enable_devices(*key, &name, network);
    }
    key_hex.zeroize();
    let _ = BLOCKER.set(core.blocker());
    let out = crate::bridge::to_json(&core.boot());
    drop(guard);
    reply(&mut env, &out)
}

/// A `MobileCommand` of the shell interface.
#[no_mangle]
pub extern "system" fn Java_app_blackhole_browser_Native_command(
    mut env: JNIEnv,
    _class: JClass,
    json: JString,
) -> jstring {
    let json = text(&mut env, &json);
    let out = with_core(|c| c.command_json(&json));
    reply(&mut env, &out)
}

/// A `NativeEvent` of the shell.
#[no_mangle]
pub extern "system" fn Java_app_blackhole_browser_Native_event(
    mut env: JNIEnv,
    _class: JClass,
    json: JString,
) -> jstring {
    let json = text(&mut env, &json);
    let out = with_core(|c| c.event_json(&json));
    reply(&mut env, &out)
}

/// A `PageEvent` posted by the page of `tab` (untrusted).
#[no_mangle]
pub extern "system" fn Java_app_blackhole_browser_Native_pageEvent(
    mut env: JNIEnv,
    _class: JClass,
    tab: jlong,
    json: JString,
) -> jstring {
    let json = text(&mut env, &json);
    let tab = u64::try_from(tab).unwrap_or(0);
    let out = with_core(|c| c.page_event_json(tab, &json));
    reply(&mut env, &out)
}

/// Whether to refuse a subresource request (any thread).
#[no_mangle]
pub extern "system" fn Java_app_blackhole_browser_Native_shouldBlock(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
    page: JString,
) -> jboolean {
    let (url, page) = (text(&mut env, &url), text(&mut env, &page));
    match BLOCKER.get() {
        Some(b) if b.should_block(&url, &page) => JNI_TRUE,
        _ => JNI_FALSE,
    }
}
