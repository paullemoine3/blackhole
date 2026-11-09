//! Camera, microphone and location for sites: the shell reports a site's request
//! (`onPermissionRequest`, `onGeolocationPermissionsShowPrompt`), the interface asks the user,
//! and only the interface's answer (`MobileCommand::PermissionAnswer`) grants it. A page never
//! answers for itself. An allowed site keeps its permission until the app closes (memory only,
//! never for private tabs); a tab that is not on screen is always refused.

use crate::bridge::Effect;
use crate::core::Core;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use url::Url;

/// What a site may ask for; anything else is refused.
pub(crate) const KINDS: [&str; 4] = ["camera", "microphone", "location", "protected_media"];

#[derive(Debug, Default)]
pub(crate) struct Permissions {
    /// Shell request id → (tab, origin, kinds), waiting for the user's answer.
    pending: HashMap<u64, (u64, String, Vec<String>)>,
    /// (origin, kind) allowed until the app closes.
    granted: HashSet<(String, String)>,
}

impl Permissions {
    pub(crate) fn clear(&mut self) {
        self.granted.clear();
    }
}

/// `https://host[:port]` of a secure origin (or local http), `None` otherwise.
fn secure_origin(origin: &str) -> Option<String> {
    let url = Url::parse(origin).ok()?;
    let host = url.host_str()?;
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    (url.scheme() == "https" || url.scheme() == "http" && local).then(|| url.origin().ascii_serialization())
}

impl Core {
    pub(crate) fn permission_asked(&mut self, id: u64, tab: u64, origin: &str, kinds: &[String]) -> Vec<Effect> {
        let deny = vec![Effect::Permission { id, allow: false }];
        let Some(origin) = secure_origin(origin) else {
            return deny;
        };
        let mut wanted: Vec<String> = Vec::new();
        for k in kinds {
            if !KINDS.contains(&k.as_str()) {
                return deny;
            }
            if !wanted.contains(k) {
                wanted.push(k.clone());
            }
        }
        if wanted.is_empty() || !self.exists(tab) {
            return deny;
        }
        let private = Self::is_private(tab);
        if !private && wanted.iter().all(|k| self.permissions.granted.contains(&(origin.clone(), k.clone()))) {
            return vec![Effect::Permission { id, allow: true }];
        }
        if self.current != Some(tab) {
            return deny;
        }
        let host = Url::parse(&origin).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
        self.permissions.pending.insert(id, (tab, origin, wanted.clone()));
        vec![Effect::Ui {
            msg: json!({"type": "permission", "id": id, "host": host, "kinds": wanted}),
        }]
    }

    pub(crate) fn permission_answer(&mut self, id: u64, allow: bool) -> Vec<Effect> {
        let Some((tab, origin, kinds)) = self.permissions.pending.remove(&id) else {
            return Vec::new();
        };
        if allow && !Self::is_private(tab) {
            for k in kinds {
                self.permissions.granted.insert((origin.clone(), k));
            }
        }
        vec![Effect::Permission { id, allow }]
    }

    /// A closed tab's requests are refused.
    pub(crate) fn permissions_of_closed(&mut self, tab: u64) -> Vec<Effect> {
        let ids: Vec<u64> = self.permissions.pending.iter().filter(|(_, p)| p.0 == tab).map(|(id, _)| *id).collect();
        ids.into_iter()
            .map(|id| {
                self.permissions.pending.remove(&id);
                Effect::Permission { id, allow: false }
            })
            .collect()
    }
}
