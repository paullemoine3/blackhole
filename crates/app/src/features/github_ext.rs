//! Settings > Extensions > Add from GitHub (market step 3): the steps of adding an extension
//! that BlackHole did not review, each one shown to the person before the next. Network work
//! runs on threads; their answers carry the number of the flow they belong to, so an answer to
//! a flow the person left (Cancel, another repository) is dropped. Rules and downloads:
//! `blackhole_infrastructure::github_ext`, `domain::market`. Settings > Extensions > Add from a
//! link (0.8.25) runs the same steps for a package offered by an address (`.crx`, `.zip`,
//! `.xpi`, or a Chrome Web Store / Edge Add-ons page): no release to choose, no updates.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{channel, Receiver, Sender},
};

use blackhole_domain::market::{PermissionLine, self as rules};
use blackhole_infrastructure::github_ext::{Pinned, Prepared, Release, self as gh};
use serde_json::{json, Value};

use crate::{i18n, platform::now_secs, state::Running};

pub enum Flow {
    Idle,
    Looking { repo: String },
    Choose { release: Release, in_market: Option<String> },
    Preparing { release: Release, asset: String },
    /// A package asked from a link is being downloaded and checked.
    Fetching { url: url::Url },
    Ready { prepared: Box<Prepared>, added: Vec<PermissionLine> },
    Installing { id: String, name: String },
    Done { id: String, name: String },
    Failed { why: String },
}

enum Msg {
    Looked(u64, Result<Release, String>),
    Prepared(u64, Result<Prepared, String>),
    Installed(u64, Result<(Pinned, PathBuf), String>),
    Update(String, Result<Release, String>),
    /// An extension offered by a paired device (0.8.25), installed on the person's word.
    Synced(String, String, Result<(Pinned, PathBuf), String>),
}

pub struct Sideload {
    pub pinned: Vec<Pinned>,
    pub flow: Flow,
    seq: u64,
    /// Newer releases found for pinned extensions: id → (release, file of the same kind).
    pub updates: HashMap<String, (Release, String)>,
    /// Update checks still running ("Check for updates").
    pub checking: usize,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Sideload {
    pub fn new(data: &std::path::Path) -> Self {
        let (tx, rx) = channel();
        Self {
            pinned: gh::load(data),
            flow: Flow::Idle,
            seq: 0,
            updates: HashMap::new(),
            checking: 0,
            tx,
            rx,
        }
    }

    pub fn find(&self, id: &str) -> Option<&Pinned> {
        self.pinned.iter().find(|p| p.id == id)
    }

    fn next(&mut self, flow: Flow) -> u64 {
        self.seq += 1;
        self.flow = flow;
        self.seq
    }
}

fn lines(lines: &[PermissionLine]) -> Vec<Value> {
    lines
        .iter()
        .map(|l| json!({"key": l.key, "sensitive": l.sensitive, "items": l.items}))
        .collect()
}

fn platform() -> &'static str {
    rules::platform_key(std::env::consts::OS).unwrap_or("")
}

impl Running {
    /// The add-from-GitHub panel as the Settings page draws it (`null` when closed).
    pub(crate) fn github_json(&self) -> Value {
        let g = &self.github;
        let flow = match &g.flow {
            Flow::Idle => Value::Null,
            Flow::Looking { repo } => json!({"stage": "looking", "repo": repo}),
            Flow::Choose { release, in_market } => {
                let files = release.for_platform(platform());
                json!({
                    "stage": "choose", "repo": format!("{}/{}", release.owner, release.repo), "tag": release.tag,
                    "inMarket": in_market,
                    "assets": files.iter().enumerate().map(|(i, (a, _))| json!({
                        "name": a.name, "size": a.size, "digest": a.digest.is_some(), "recommended": i == 0,
                    })).collect::<Vec<_>>(),
                })
            }
            Flow::Preparing { release, asset } => json!({
                "stage": "preparing", "repo": format!("{}/{}", release.owner, release.repo), "tag": release.tag, "asset": asset,
            }),
            Flow::Fetching { url } => json!({
                "stage": "preparing", "direct": true, "repo": url.host_str(), "asset": rules::direct_file_name(url),
            }),
            Flow::Ready { prepared: p, added } => {
                let before = g.find(&p.id);
                json!({
                    "stage": "ready", "id": p.id, "tag": p.tag, "direct": p.direct, "url": p.url,
                    "repo": if p.direct { p.owner.clone() } else { format!("{}/{}", p.owner, p.repo) },
                    "name": p.name, "version": p.version, "file": p.asset, "size": p.size, "sha256": p.sha256,
                    "digest": p.digest_checked,
                    "permissions": lines(&rules::permission_lines(&p.permissions)),
                    "added": lines(added),
                    "update": before.map(|b| json!({"version": b.version, "sha256": b.sha256})),
                })
            }
            Flow::Installing { name, .. } => json!({"stage": "installing", "name": name}),
            Flow::Done { id, name } => json!({"stage": "done", "id": id, "name": name}),
            Flow::Failed { why } => json!({"stage": "error", "error": i18n::render(why)}),
        };
        json!({ "flow": flow, "checking": g.checking > 0 })
    }

    /// Extensions added from GitHub, in the market's list (marked unverified).
    pub(crate) fn github_entries(&self) -> Vec<(blackhole_infrastructure::market::Entry, &Pinned)> {
        self.github.pinned.iter().map(|p| (p.entry(), p)).collect()
    }

    /// Looks up the latest release of what the person typed.
    pub(crate) fn github_lookup(&mut self, input: &str) {
        let Some((owner, repo)) = rules::github_repo(input) else {
            self.github.next(Flow::Failed { why: i18n::message("msg.github_bad_repo", &[]) });
            return self.ui_state();
        };
        let seq = self.github.next(Flow::Looking { repo: format!("{owner}/{repo}") });
        let tx = self.github.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Looked(seq, gh::latest_release(&owner, &repo)));
        });
        self.ui_state();
    }

    /// Add from a link: downloads and checks the package at the address typed.
    pub(crate) fn link_prepare(&mut self, input: &str) {
        let Some(url) = rules::direct_package_url(input) else {
            self.github.next(Flow::Failed { why: i18n::message("msg.link_bad_url", &[]) });
            return self.ui_state();
        };
        let seq = self.github.next(Flow::Fetching { url: url.clone() });
        let tx = self.github.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Prepared(seq, gh::prepare_direct(&url)));
        });
        self.ui_state();
    }

    /// Downloads and checks the file the person picked (or the update found for `update`).
    pub(crate) fn github_prepare(&mut self, asset: Option<String>, update: Option<String>) {
        let (release, asset) = match (update, std::mem::replace(&mut self.github.flow, Flow::Idle)) {
            (Some(id), flow) => {
                self.github.flow = flow;
                match self.github.updates.get(&id) {
                    Some((release, asset)) => (release.clone(), asset.clone()),
                    None => return,
                }
            }
            (None, Flow::Choose { release, .. }) => match asset {
                Some(asset) if release.assets.iter().any(|a| a.name == asset) => (release, asset),
                _ => {
                    self.github.flow = Flow::Choose { release, in_market: None };
                    return;
                }
            },
            (None, flow) => {
                self.github.flow = flow;
                return;
            }
        };
        let seq = self.github.next(Flow::Preparing { release: release.clone(), asset: asset.clone() });
        let tx = self.github.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Prepared(seq, gh::prepare(&release, &asset)));
        });
        self.ui_state();
    }

    /// The person accepted the package shown: store it, pin it, load it.
    pub(crate) fn github_install(&mut self) {
        let Flow::Ready { prepared, .. } = std::mem::replace(&mut self.github.flow, Flow::Idle) else {
            return;
        };
        if self.webext.is_none() {
            return;
        }
        self.installing.insert(prepared.id.clone());
        let seq = self.github.next(Flow::Installing { id: prepared.id.clone(), name: prepared.name.clone() });
        let (tx, data) = (self.github.tx.clone(), self.data.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Installed(seq, gh::install(&data, &prepared, now_secs())));
        });
        self.ui_state();
    }

    pub(crate) fn github_cancel(&mut self) {
        self.github.next(Flow::Idle);
        self.ui_state();
    }

    /// Looks for a newer release of every pinned extension ("Check for updates").
    pub(crate) fn github_check_updates(&mut self) {
        if self.github.checking > 0 {
            return;
        }
        // Added from a link: nothing to compare with, no updates.
        for p in self.github.pinned.clone().into_iter().filter(|p| !p.direct) {
            self.github.checking += 1;
            let tx = self.github.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Update(p.id.clone(), gh::latest_release(&p.owner, &p.repo)));
            });
        }
    }

    /// An extension of a paired device that BlackHole did not review (GitHub, link), added here
    /// after the person said so in the notice of the sync (0.8.25): the latest release's best
    /// file for this system, or the same link, checked like any other before it is stored.
    pub(crate) fn sync_install(&mut self, offer: &blackhole_domain::ext_sync::Offer) {
        use blackhole_domain::ext_sync::Source;
        if self.webext.is_none() || !self.installing.insert(offer.id.clone()) {
            return;
        }
        let (tx, data, id, name, source) = (self.github.tx.clone(), self.data.clone(), offer.id.clone(), offer.name.clone(), offer.source.clone());
        std::thread::spawn(move || {
            let prepared = match source {
                Source::Github { owner, repo } => gh::latest_release(&owner, &repo).and_then(|release| {
                    let best = release.for_platform(platform()).first().map(|(a, _)| a.name.clone());
                    let best = best.ok_or_else(|| i18n::message("msg.github_no_package", &[]))?;
                    gh::prepare(&release, &best)
                }),
                Source::Link { url } => url::Url::parse(&url).map_err(|e| e.to_string()).and_then(|u| gh::prepare_direct(&u)),
                Source::Market => Err(String::new()),
            };
            let result = prepared.and_then(|p| gh::install(&data, &p, now_secs()));
            let _ = tx.send(Msg::Synced(id, name, result));
        });
    }

    pub(crate) fn github_remove(&mut self, id: &str) {
        gh::remove(&self.data, id);
        self.github.pinned.retain(|p| p.id != id);
        self.github.updates.remove(id);
    }

    /// Applies what the threads report.
    pub(crate) fn poll_github(&mut self) {
        while let Ok(msg) = self.github.rx.try_recv() {
            let current = self.github.seq;
            match msg {
                Msg::Looked(seq, _) | Msg::Prepared(seq, _) | Msg::Installed(seq, _) if seq != current => {}
                Msg::Looked(_, Ok(release)) => {
                    let repo = format!("{}/{}", release.owner, release.repo);
                    // The same repository in the market: the reviewed entry is the one to install.
                    let in_market = self
                        .market.index
                        .extensions
                        .iter()
                        .find(|e| e.source.as_ref().is_some_and(|s| s.repo.eq_ignore_ascii_case(&repo)))
                        .map(|e| e.id.clone());
                    self.github.flow = if release.for_platform(platform()).is_empty() && in_market.is_none() {
                        Flow::Failed { why: i18n::message("msg.github_no_package", &[]) }
                    } else {
                        Flow::Choose { release, in_market }
                    };
                }
                Msg::Prepared(_, Ok(prepared)) => {
                    let added = gh::added_lines(self.github.find(&prepared.id), &prepared);
                    // Another release must be a newer version (the same one may be installed again).
                    let older = self
                        .github
                        .find(&prepared.id)
                        .is_some_and(|p| p.tag != prepared.tag && !rules::is_upgrade(&prepared.version, Some(&p.version)));
                    self.github.flow = if older {
                        Flow::Failed { why: i18n::message("msg.github_not_newer", &[]) }
                    } else {
                        Flow::Ready { prepared: Box::new(prepared), added }
                    };
                }
                Msg::Installed(_, Ok((pinned, path))) => {
                    self.installing.remove(&pinned.id);
                    self.install_errors.remove(&pinned.id);
                    self.svc.set_enabled_flag(&pinned.id, true);
                    if let Some(host) = &self.webext {
                        host.load(&pinned.id, &path);
                    }
                    self.github.updates.remove(&pinned.id);
                    self.github.flow = Flow::Done { id: pinned.id.clone(), name: pinned.name.clone() };
                    self.github.pinned.retain(|p| p.id != pinned.id);
                    self.github.pinned.push(pinned);
                }
                Msg::Installed(_, Err(why)) => {
                    if let Flow::Installing { id, .. } = &self.github.flow {
                        self.installing.remove(id);
                    }
                    self.github.flow = Flow::Failed { why };
                }
                Msg::Looked(_, Err(why)) | Msg::Prepared(_, Err(why)) => {
                    self.github.flow = Flow::Failed { why };
                }
                Msg::Synced(id, name, result) => {
                    self.installing.remove(&id);
                    match result {
                        Ok((pinned, path)) => {
                            self.install_errors.remove(&pinned.id);
                            self.svc.set_enabled_flag(&pinned.id, true);
                            if let Some(host) = &self.webext {
                                host.load(&pinned.id, &path);
                            }
                            self.github.pinned.retain(|p| p.id != pinned.id);
                            self.github.pinned.push(pinned);
                            self.toast(&i18n::tf("toast.ext_synced", &[("name", &name)]));
                        }
                        Err(why) => {
                            tracing::warn!("{id}: {why}");
                            self.notify(
                                crate::shell::notices::Notice::Problem,
                                &i18n::tf("toast.ext_sync_failed", &[("name", &name)]),
                                Value::Null,
                            );
                        }
                    }
                }
                Msg::Update(id, result) => {
                    self.github.checking = self.github.checking.saturating_sub(1);
                    match result.map(|release| {
                        let asset = self.github.find(&id).and_then(|p| gh::update_of(p, &release));
                        (release, asset)
                    }) {
                        Ok((release, Some(asset))) => {
                            self.github.updates.insert(id, (release, asset));
                        }
                        Ok((_, None)) => {
                            self.github.updates.remove(&id);
                        }
                        Err(why) => tracing::warn!("{id}: {why}"),
                    }
                    if self.github.checking == 0 && self.market.fetching.is_none() {
                        self.market.outdated = Some(self.outdated_extensions().len() + self.github.updates.len());
                    }
                }
            }
            self.ui_state();
        }
    }
}
