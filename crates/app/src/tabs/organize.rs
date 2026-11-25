//! Spaces, folders and quick access: create, rename, recolour, move, delete.

use blackhole_domain::{FolderId, Removed, SpaceId, TabId};
use url::Url;

use crate::i18n::{t, tf};
use crate::ipc::UiCommand;
use crate::state::Running;

impl Running {
    /// Commands that organize the sidebar: spaces, folders, names, icons and quick access.
    pub(crate) fn handle_organize(&mut self, command: UiCommand) {
        match command {
            UiCommand::DeleteSpace { id } => {
                // Its tabs go to the archive, like those of a deleted folder: nothing is lost.
                let doomed: Vec<TabId> = self
                    .svc
                    .workspace()
                    .spaces()
                    .iter()
                    .find(|s| s.id.0 == id)
                    .map(|s| s.tabs.iter().map(|t| t.id).collect())
                    .unwrap_or_default();
                if self.svc.workspace().spaces().len() > 1 {
                    for &tab in &doomed {
                        self.archive_tab(tab);
                    }
                }
                let snapshot = self.svc.workspace().snapshot_space(SpaceId(id));
                if let Some(tabs) = self.svc.delete_space(SpaceId(id)) {
                    for tab in tabs {
                        self.drop_view(tab);
                    }
                    self.home_visible = false;
                    self.easel_tab = false;
                    self.library = false;
                    self.settings_shown = false;
                    self.sync();
                    if let Some(snapshot @ Removed::Space { .. }) = snapshot {
                        let name = match &snapshot {
                            Removed::Space { space, .. } => space.name.clone(),
                            Removed::Folder { .. } | Removed::Tabs { .. } => String::new(),
                        };
                        self.toast_undo(&tf("toast.space_deleted", &[("name", &name)]), snapshot);
                    }
                }
            }
            UiCommand::SetSpaceColor { id, color } => {
                if self.svc.set_space_color(SpaceId(id), &color) {
                    self.sync();
                }
            }
            UiCommand::PinSite { url } => {
                self.open(&url);
                // The site may already be open as a quick access tab: keep it pinned.
                let tab = self.svc.workspace().active_tab().filter(|t| !t.pinned).map(|t| t.id);
                if let Some(id) = tab {
                    self.svc.toggle_pin(id);
                }
                self.sync();
            }
            UiCommand::HideFavorite { url } => {
                if let Ok(url) = Url::parse(&url) {
                    self.svc.hide_favorite(&url);
                    self.ui_home();
                }
            }
            UiCommand::PickIcon { kind, id } => {
                self.palette.icon_target = Some((kind, id));
                self.open_palette("emoji");
            }
            UiCommand::SetSpaceIcon { id, icon } => {
                self.svc.set_space_icon(SpaceId(id), &icon);
                self.ui_state();
            }
            UiCommand::SetFolderIcon { id, icon } => {
                self.svc.set_folder_icon(FolderId(id), &icon);
                self.ui_state();
            }
            UiCommand::NewFolder { parent } => {
                match parent {
                    Some(p) => {
                        let made = self.svc.add_subfolder(&t("palette.new_folder"), FolderId(p));
                        // Show the new subfolder: a collapsed parent would hide it.
                        if self
                            .svc
                            .workspace()
                            .active_space()
                            .folders
                            .iter()
                            .any(|f| f.id == FolderId(p) && f.collapsed)
                        {
                            self.svc.toggle_folder(FolderId(p));
                        }
                        self.ui_state();
                        if let Some(id) = made {
                            self.ui(format!("window.bh&&bh.editFolderId({})", id.0));
                        }
                        return;
                    }
                    None => {
                        self.svc.add_folder(&t("palette.new_folder"));
                    }
                }
                self.ui_state();
            }
            UiCommand::RenameFolder { id, name } => {
                self.svc.rename_folder(FolderId(id), &name);
                self.ui_state();
            }
            UiCommand::RenameSpace { id, name } => {
                self.svc.rename_space(SpaceId(id), &name);
                self.ui_state();
            }
            UiCommand::EditSpace { id } => self.ui(format!("window.bh&&bh.editSpace({id})")),
            UiCommand::RenameTab { id, name } => {
                self.svc.rename_tab(TabId(id), &name);
                self.ui_state();
            }
            UiCommand::EditTab { id } => self.ui(format!("window.bh&&bh.editTab({id})")),
            UiCommand::EditFolder { id } => {
                self.ui(format!("window.bh&&bh.editFolderId({id})"))
            }
            UiCommand::ToggleFolder { id } => {
                self.svc.toggle_folder(FolderId(id));
                self.ui_state();
            }
            UiCommand::MoveFolder { id, parent, before } => {
                self.svc
                    .move_folder(FolderId(id), parent.map(FolderId), before.map(FolderId));
                self.ui_state();
            }
            UiCommand::DeleteFolder { id } => {
                // The folder takes its tabs with it (they go to the archive like any closed tab).
                let doomed = self.svc.workspace().folder_tree(FolderId(id));
                let inside: Vec<TabId> = self
                    .svc
                    .workspace()
                    .active_space()
                    .tabs
                    .iter()
                    .filter(|t| t.folder.is_some_and(|f| doomed.contains(&f)))
                    .map(|t| t.id)
                    .collect();
                let snapshot = self.svc.workspace().snapshot_folder(FolderId(id));
                for tab in inside {
                    self.archive_tab(tab);
                    self.svc.close_tab(tab);
                    self.drop_view(tab);
                }
                if self.svc.delete_folder(FolderId(id)) {
                    if let Some(snapshot) = snapshot {
                        let name = match &snapshot {
                            Removed::Folder { folders, .. } => {
                                folders.first().map(|(_, f)| f.name.clone()).unwrap_or_default()
                            }
                            Removed::Space { .. } | Removed::Tabs { .. } => String::new(),
                        };
                        self.toast_undo(&tf("toast.folder_deleted", &[("name", &name)]), snapshot);
                    }
                }
                self.sync();
            }
            UiCommand::Space { id } => {
                self.svc.switch_space(SpaceId(id));
                self.home_visible = false;
                self.easel_tab = false;
                self.library = false;
                self.settings_shown = false;
                self.sync();
            }
            UiCommand::MoveSpace { id, before } => {
                self.svc.move_space(SpaceId(id), before.map(SpaceId));
                self.ui_state();
            }
            UiCommand::NewSpace { name, color } => {
                let name = if name.trim().is_empty() {
                    t("sidebar.space")
                } else {
                    name
                };
                let made = self.svc.add_space(&name, &color);
                self.pulsars_new_space(made.0);
                self.sync();
            }
            _ => {}
        }
    }
}
