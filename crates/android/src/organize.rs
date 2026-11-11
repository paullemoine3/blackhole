//! Tab and space menus, folders and tidying up: the desktop's rules (`domain::workspace`)
//! behind the phone's long-press menus. Nothing here applies to private tabs or popups.

use crate::bridge::{Effect, MobileCommand};
use crate::core::{now_unix, Core};
use blackhole_domain::{FolderId, Removed, SpaceId, Tab, TabId, Tidied};
use serde_json::{json, Value};

/// Sites with this many loose tabs become a folder, once the space has `TIDY_FROM` loose tabs
/// (the desktop's thresholds).
const TIDY_MIN: usize = 3;
const TIDY_FROM: usize = 6;
/// How long "Undo" brings back a deleted space or folder (the toast lasts 15 s).
const UNDO_FOR: i64 = 20;
const NAME_MAX: usize = 40;

#[derive(Debug, Default)]
pub(crate) struct Organize {
    removed: Option<(Removed, i64)>,
    tidied: Option<(Tidied, i64)>,
    /// The tidy card was dismissed (until the app restarts).
    tidy_dismissed: bool,
}

fn name_of(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX).collect()
}

impl Core {
    /// A tab of the space on screen (never a private tab or a popup).
    fn space_tab(&self, tab: u64) -> Option<&Tab> {
        self.svc.workspace().active_space().tabs.iter().find(|t| t.id == TabId(tab))
    }

    fn undo_toast(&self, text: &str, action: &str) -> Effect {
        Effect::Ui {
            msg: json!({"type": "toast", "text": text, "undo": false, "action": action}),
        }
    }

    /// The workspace changed under the tab on screen: what it now shows.
    fn show_active(&mut self) -> Vec<Effect> {
        match self.svc.workspace().active_tab().map(|t| t.id.0) {
            Some(tab) => self.show(tab, None),
            None => self.home(),
        }
    }

    /// The card that offers to file loose tabs by site, `null` when there is nothing to offer.
    pub(crate) fn tidy_json(&self) -> Value {
        let ws = self.svc.workspace();
        let loose = ws.active_space().tabs.iter().filter(|t| !t.pinned && t.folder.is_none()).count();
        if self.organize.tidy_dismissed || loose < TIDY_FROM {
            return Value::Null;
        }
        let groups = ws.tidy_suggestion(TIDY_MIN);
        if groups.is_empty() {
            return Value::Null;
        }
        json!({"tabs": groups.iter().map(|g| g.tabs.len()).sum::<usize>(), "folders": groups.len()})
    }

    pub(crate) fn organize(&mut self, command: MobileCommand) -> Vec<Effect> {
        use MobileCommand as C;
        let mut out = Vec::new();
        match command {
            C::RenameTab { tab, name } if self.space_tab(tab).is_some() => {
                self.svc.rename_tab(TabId(tab), &name_of(&name));
            }
            C::MoveTabToSpace { tab, space } => {
                let ws = self.svc.workspace();
                let Some(name) = ws.spaces().iter().find(|s| s.id == SpaceId(space)).map(|s| s.name.clone()) else {
                    return out;
                };
                if self.space_tab(tab).is_none() || ws.active_space().id == SpaceId(space) {
                    return out;
                }
                let shown = self.current == Some(tab);
                if !self.svc.move_tab_to_space(TabId(tab), SpaceId(space)) {
                    return out;
                }
                // Its view goes: the tab loads again on the cookies of its new space.
                out.extend(self.forget_view(tab));
                if shown {
                    out.extend(self.show_active());
                }
                out.push(Self::toast(&self.tf("moved_to_space", &[("name", &name)]), false));
            }
            C::MoveTabToFolder { tab, folder } if self.space_tab(tab).is_some() => {
                self.svc.move_tab(TabId(tab), None, folder.map(FolderId), false);
            }
            C::CloseOtherTabs { tab } if self.space_tab(tab).is_some() => {
                // The tab kept comes on screen first: no other one is shown on the way.
                if self.current != Some(tab) {
                    self.svc.activate_tab(TabId(tab));
                    out.extend(self.show(tab, None));
                }
                let others: Vec<u64> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.id != TabId(tab) && !t.pinned && t.folder.is_none())
                    .map(|t| t.id.0)
                    .collect();
                for &other in &others {
                    out.extend(self.close(other).into_iter().filter(
                        |e| !matches!(e, Effect::Ui { msg } if msg["type"] == "toast" || msg["type"] == "state"),
                    ));
                }
                if !others.is_empty() {
                    out.push(Self::toast(&self.tn("tabs_closed", others.len()), false));
                }
            }
            C::RenameSpace { space, name } if !name_of(&name).is_empty() => {
                self.svc.rename_space(SpaceId(space), &name_of(&name));
            }
            C::DeleteSpace { space } => {
                let ws = self.svc.workspace();
                // The last space stays.
                let (true, Some(snapshot)) = (ws.spaces().len() > 1, ws.snapshot_space(SpaceId(space))) else {
                    return out;
                };
                let name = match &snapshot {
                    Removed::Space { space, .. } => space.name.clone(),
                    Removed::Folder { .. } => String::new(),
                };
                let was_active = ws.active_space().id == SpaceId(space);
                let Some(tabs) = self.svc.delete_space(SpaceId(space)) else {
                    return out;
                };
                for tab in tabs {
                    out.extend(self.forget_view(tab.0));
                }
                if was_active && !self.current.is_some_and(Self::is_private) {
                    out.extend(self.show_active());
                }
                self.organize.removed = Some((snapshot, now_unix()));
                out.push(self.undo_toast(&self.tf("space_deleted", &[("name", &name)]), "undo_removal"));
            }
            C::UndoRemoval => {
                if let Some((removed, at)) = self.organize.removed.take() {
                    if now_unix() - at <= UNDO_FOR {
                        self.svc.undo_removal(removed);
                    }
                }
            }
            C::AddFolder { name, tab } => {
                let name = name_of(&name);
                let name = if name.is_empty() { self.t("new_folder") } else { name };
                let folder = self.svc.add_folder(&name);
                if let Some(tab) = tab.filter(|t| self.space_tab(*t).is_some()) {
                    self.svc.move_tab(TabId(tab), None, Some(folder), false);
                }
            }
            C::RenameFolder { folder, name } if !name_of(&name).is_empty() => {
                self.svc.rename_folder(FolderId(folder), &name_of(&name));
            }
            C::ToggleFolder { folder } => {
                self.svc.toggle_folder(FolderId(folder));
            }
            C::DeleteFolder { folder } => {
                let ws = self.svc.workspace();
                let Some(snapshot) = ws.snapshot_folder(FolderId(folder)) else {
                    return out;
                };
                let name = match &snapshot {
                    Removed::Folder { folders, .. } => folders.first().map(|(_, f)| f.name.clone()).unwrap_or_default(),
                    Removed::Space { .. } => String::new(),
                };
                // A folder goes with what it holds, like on the desktop.
                let tree = ws.folder_tree(FolderId(folder));
                let inside: Vec<u64> = ws
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.folder.is_some_and(|f| tree.contains(&f)))
                    .map(|t| t.id.0)
                    .collect();
                let shown = self.current.is_some_and(|t| inside.contains(&t));
                for tab in inside {
                    self.svc.close_tab(TabId(tab));
                    out.extend(self.forget_view(tab));
                }
                self.svc.delete_folder(FolderId(folder));
                if shown {
                    out.extend(self.show_active());
                }
                self.organize.removed = Some((snapshot, now_unix()));
                out.push(self.undo_toast(&self.tf("folder_deleted", &[("name", &name)]), "undo_removal"));
            }
            C::Tidy => {
                let groups = self.svc.workspace().tidy_suggestion(TIDY_MIN);
                if groups.is_empty() {
                    return out;
                }
                let done = self.svc.tidy(&groups);
                let text = self.tn("tabs_tidied", done.tabs());
                self.organize.tidied = Some((done, now_unix()));
                out.push(self.undo_toast(&text, "untidy"));
            }
            C::Untidy => {
                if let Some((done, at)) = self.organize.tidied.take() {
                    if now_unix() - at <= UNDO_FOR {
                        self.svc.untidy(&done);
                    }
                }
            }
            C::DismissTidy => self.organize.tidy_dismissed = true,
            _ => return out,
        }
        out.push(self.state());
        out
    }
}
