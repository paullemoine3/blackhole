use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpaceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FolderId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub url: Url,
    pub pinned: bool,
    pub folder: Option<FolderId>,
}

/// A named group of tabs inside a space.
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    /// Optional emoji shown instead of the default folder glyph.
    pub icon: String,
    pub collapsed: bool,
}

/// An Arc-style "Space": an isolated group of tabs with its own identity color.
#[derive(Debug, Clone, PartialEq)]
pub struct Space {
    pub id: SpaceId,
    pub name: String,
    pub color: String,
    /// Optional emoji shown on the space switcher.
    pub icon: String,
    pub tabs: Vec<Tab>,
    pub folders: Vec<Folder>,
    pub active_tab: Option<TabId>,
    /// Second pane of the split view, shown next to the active tab.
    pub split: Option<TabId>,
}

/// Keeps a single emoji-like symbol: no control characters, at most one ZWJ/flag-sized cluster.
pub fn sanitize_icon(input: &str) -> String {
    let icon: String = input
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(8)
        .collect();
    if icon.chars().any(|c| c.is_ascii_alphanumeric()) {
        String::new()
    } else {
        icon
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    spaces: Vec<Space>,
    active_space: SpaceId,
    next_id: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    pub fn new() -> Self {
        let mut ws = Self {
            spaces: Vec::new(),
            active_space: SpaceId(0),
            next_id: 1,
        };
        ws.active_space = ws.add_space("Personal", "#7c5cff");
        ws
    }

    /// Rebuilds a workspace from persisted data, guaranteeing its invariants.
    pub fn restore(spaces: Vec<Space>, active_space: SpaceId) -> Self {
        if spaces.is_empty() {
            return Self::new();
        }
        let next_id = spaces
            .iter()
            .flat_map(|s| {
                std::iter::once(s.id.0)
                    .chain(s.tabs.iter().map(|t| t.id.0))
                    .chain(s.folders.iter().map(|f| f.id.0))
            })
            .max()
            .unwrap_or(0)
            + 1;
        let active_space = if spaces.iter().any(|s| s.id == active_space) {
            active_space
        } else {
            spaces[0].id
        };
        Self {
            spaces,
            active_space,
            next_id,
        }
    }

    pub fn spaces(&self) -> &[Space] {
        &self.spaces
    }

    pub fn active_space(&self) -> &Space {
        self.spaces
            .iter()
            .find(|s| s.id == self.active_space)
            .expect("active space exists")
    }

    fn active_space_mut(&mut self) -> &mut Space {
        let id = self.active_space;
        self.spaces
            .iter_mut()
            .find(|s| s.id == id)
            .expect("active space exists")
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let space = self.active_space();
        space.tabs.iter().find(|t| Some(t.id) == space.active_tab)
    }

    pub fn add_space(&mut self, name: &str, color: &str) -> SpaceId {
        let id = SpaceId(self.alloc());
        self.spaces.push(Space {
            id,
            name: name.trim().to_owned(),
            color: color.to_owned(),
            icon: String::new(),
            tabs: Vec::new(),
            folders: Vec::new(),
            active_tab: None,
            split: None,
        });
        id
    }

    /// Deletes a space and returns the ids of its tabs. The last space cannot be deleted; when
    /// the active one goes, its left neighbour (or the first space) takes over.
    pub fn delete_space(&mut self, id: SpaceId) -> Option<Vec<TabId>> {
        if self.spaces.len() < 2 {
            return None;
        }
        let pos = self.spaces.iter().position(|s| s.id == id)?;
        let removed = self.spaces.remove(pos);
        if self.active_space == id {
            self.active_space = self.spaces[pos.saturating_sub(1)].id;
        }
        Some(removed.tabs.iter().map(|t| t.id).collect())
    }

    /// Drag & drop: places a space before `before` (or last). The active space is unchanged.
    pub fn move_space(&mut self, id: SpaceId, before: Option<SpaceId>) -> bool {
        let Some(pos) = self.spaces.iter().position(|s| s.id == id) else {
            return false;
        };
        let space = self.spaces.remove(pos);
        let at = before
            .filter(|b| *b != id)
            .and_then(|b| self.spaces.iter().position(|s| s.id == b))
            .unwrap_or(self.spaces.len());
        self.spaces.insert(at, space);
        true
    }

    pub fn set_space_icon(&mut self, id: SpaceId, icon: &str) -> bool {
        match self.spaces.iter_mut().find(|s| s.id == id) {
            Some(space) => {
                space.icon = sanitize_icon(icon);
                true
            }
            None => false,
        }
    }

    pub fn set_folder_icon(&mut self, id: FolderId, icon: &str) -> bool {
        match self
            .active_space_mut()
            .folders
            .iter_mut()
            .find(|f| f.id == id)
        {
            Some(folder) => {
                folder.icon = sanitize_icon(icon);
                true
            }
            None => false,
        }
    }

    /// The tab displayed beside the active one, when the split view is valid.
    pub fn split_tab(&self) -> Option<TabId> {
        let space = self.active_space();
        let id = space.split?;
        (Some(id) != space.active_tab && space.tabs.iter().any(|t| t.id == id)).then_some(id)
    }

    /// Opens `id` as the second pane next to the active tab.
    pub fn set_split(&mut self, id: TabId) -> bool {
        let space = self.active_space_mut();
        if space.active_tab == Some(id) || !space.tabs.iter().any(|t| t.id == id) {
            return false;
        }
        space.split = Some(id);
        true
    }

    pub fn clear_split(&mut self) {
        self.active_space_mut().split = None;
    }

    pub fn switch_space(&mut self, id: SpaceId) -> bool {
        let found = self.spaces.iter().any(|s| s.id == id);
        if found {
            self.active_space = id;
        }
        found
    }

    /// A tab of the active space already showing `url` (fragment ignored); spaces are isolated.
    pub fn find_tab(&self, url: &Url) -> Option<TabId> {
        let mut wanted = url.clone();
        wanted.set_fragment(None);
        self.active_space().tabs.iter().find_map(|t| {
            let mut current = t.url.clone();
            current.set_fragment(None);
            (current == wanted).then_some(t.id)
        })
    }

    pub fn open_tab(&mut self, url: Url) -> TabId {
        let id = TabId(self.alloc());
        let title = url.host_str().unwrap_or("New tab").to_owned();
        let space = self.active_space_mut();
        space.tabs.push(Tab {
            id,
            title,
            url,
            pinned: false,
            folder: None,
        });
        space.active_tab = Some(id);
        id
    }

    pub fn activate_tab(&mut self, id: TabId) -> bool {
        let space = self.active_space_mut();
        let found = space.tabs.iter().any(|t| t.id == id);
        if found {
            // Clicking the second pane swaps the two panes.
            if space.split == Some(id) {
                space.split = space.active_tab;
            }
            space.active_tab = Some(id);
        }
        found
    }

    /// Closes a tab in the active space; focus moves to the previous neighbour.
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let space = self.active_space_mut();
        let Some(pos) = space.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        space.tabs.remove(pos);
        if space.split == Some(id) {
            space.split = None;
        }
        if space.active_tab == Some(id) {
            let next = pos
                .saturating_sub(1)
                .min(space.tabs.len().saturating_sub(1));
            space.active_tab = space.tabs.get(next).map(|t| t.id);
        }
        true
    }

    pub fn toggle_pin(&mut self, id: TabId) -> bool {
        let space = self.active_space_mut();
        let Some(tab) = space.tabs.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        tab.pinned = !tab.pinned;
        tab.folder = None;
        space.tabs.sort_by_key(|t| !t.pinned); // stable: pinned first, order kept
        true
    }

    pub fn add_folder(&mut self, name: &str) -> FolderId {
        let id = FolderId(self.alloc());
        let name = if name.trim().is_empty() {
            "Folder"
        } else {
            name.trim()
        };
        self.active_space_mut().folders.push(Folder {
            id,
            name: name.to_owned(),
            icon: String::new(),
            collapsed: false,
        });
        id
    }

    pub fn rename_folder(&mut self, id: FolderId, name: &str) -> bool {
        let name = name.trim();
        match self
            .active_space_mut()
            .folders
            .iter_mut()
            .find(|f| f.id == id)
        {
            Some(f) if !name.is_empty() => {
                f.name = name.to_owned();
                true
            }
            _ => false,
        }
    }

    pub fn toggle_folder(&mut self, id: FolderId) -> bool {
        match self
            .active_space_mut()
            .folders
            .iter_mut()
            .find(|f| f.id == id)
        {
            Some(f) => {
                f.collapsed = !f.collapsed;
                true
            }
            None => false,
        }
    }

    /// Removes a folder; its tabs stay open, back in the ungrouped list.
    pub fn delete_folder(&mut self, id: FolderId) -> bool {
        let space = self.active_space_mut();
        let before = space.folders.len();
        space.folders.retain(|f| f.id != id);
        for tab in space.tabs.iter_mut().filter(|t| t.folder == Some(id)) {
            tab.folder = None;
        }
        space.folders.len() != before
    }

    /// Drag & drop: places a tab before `before` (or last), in `folder`, pinned or not.
    pub fn move_tab(
        &mut self,
        id: TabId,
        before: Option<TabId>,
        folder: Option<FolderId>,
        pinned: bool,
    ) -> bool {
        let space = self.active_space_mut();
        let Some(pos) = space.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let folder = folder.filter(|f| !pinned && space.folders.iter().any(|x| x.id == *f));
        let mut tab = space.tabs.remove(pos);
        tab.pinned = pinned;
        tab.folder = folder;
        let at = before
            .filter(|b| *b != id)
            .and_then(|b| space.tabs.iter().position(|t| t.id == b))
            .unwrap_or(space.tabs.len());
        space.tabs.insert(at, tab);
        space.tabs.sort_by_key(|t| !t.pinned);
        true
    }

    pub fn update_page(&mut self, id: TabId, url: Option<Url>, title: Option<String>) {
        let tab = self
            .spaces
            .iter_mut()
            .flat_map(|s| s.tabs.iter_mut())
            .find(|t| t.id == id);
        if let Some(tab) = tab {
            if let Some(url) = url {
                tab.url = url;
            }
            if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
                tab.title = title;
            }
        }
    }

    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn finds_an_open_tab_by_url_ignoring_fragment() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test/page"));
        ws.open_tab(u("https://b.test"));
        assert_eq!(ws.find_tab(&u("https://a.test/page#top")), Some(a));
        assert_eq!(ws.find_tab(&u("https://c.test")), None);
        let other = ws.add_space("Other", "#fff");
        ws.switch_space(other);
        assert_eq!(ws.find_tab(&u("https://a.test/page")), None);
    }

    #[test]
    fn closing_active_tab_focuses_neighbour() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        assert!(ws.close_tab(b));
        assert_eq!(ws.active_tab().unwrap().id, a);
        assert!(ws.close_tab(a));
        assert!(ws.active_tab().is_none());
    }

    #[test]
    fn deleting_a_space_returns_its_tabs_and_moves_focus() {
        let mut ws = Workspace::new();
        let first = ws.active_space().id;
        let second = ws.add_space("Work", "#fff");
        ws.switch_space(second);
        let tab = ws.open_tab(u("https://a.test"));
        assert_eq!(ws.delete_space(second), Some(vec![tab]));
        assert_eq!(ws.active_space().id, first);
        assert_eq!(ws.spaces().len(), 1);
    }

    #[test]
    fn spaces_can_be_reordered() {
        let mut ws = Workspace::new();
        let a = ws.active_space().id;
        let b = ws.add_space("B", "#fff");
        let c = ws.add_space("C", "#fff");
        assert!(ws.move_space(c, Some(a)));
        let ids = |ws: &Workspace| ws.spaces().iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(&ws), vec![c, a, b]);
        assert!(ws.move_space(c, None));
        assert_eq!(ids(&ws), vec![a, b, c]);
        assert_eq!(ws.active_space().id, a);
        assert!(!ws.move_space(SpaceId(999), None));
    }

    #[test]
    fn the_last_space_cannot_be_deleted() {
        let mut ws = Workspace::new();
        let only = ws.active_space().id;
        assert_eq!(ws.delete_space(only), None);
        assert_eq!(ws.delete_space(SpaceId(999)), None);
    }

    #[test]
    fn pinned_tabs_float_to_top() {
        let mut ws = Workspace::new();
        ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        ws.toggle_pin(b);
        assert_eq!(ws.active_space().tabs[0].id, b);
    }

    #[test]
    fn drag_and_drop_reorders_and_files_tabs_in_folders() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        let c = ws.open_tab(u("https://c.test"));
        assert!(ws.move_tab(c, Some(a), None, false));
        let order: Vec<_> = ws.active_space().tabs.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![c, a, b]);

        let f = ws.add_folder("Work");
        assert!(ws.move_tab(a, None, Some(f), false));
        assert_eq!(
            ws.active_space()
                .tabs
                .iter()
                .find(|t| t.id == a)
                .unwrap()
                .folder,
            Some(f)
        );

        // pinning leaves the folder; deleting a folder keeps its tabs
        assert!(ws.move_tab(a, None, Some(f), true));
        assert_eq!(ws.active_space().tabs[0].id, a);
        assert_eq!(ws.active_space().tabs[0].folder, None);
        assert!(ws.move_tab(b, None, Some(f), false));
        assert!(ws.delete_folder(f));
        assert_eq!(
            ws.active_space()
                .tabs
                .iter()
                .find(|t| t.id == b)
                .unwrap()
                .folder,
            None
        );
    }

    #[test]
    fn spaces_and_folders_keep_only_emoji_icons() {
        let mut ws = Workspace::new();
        let space = ws.active_space().id;
        assert!(ws.set_space_icon(space, " 🚀 "));
        assert_eq!(ws.active_space().icon, "🚀");
        assert!(ws.set_space_icon(space, "abc"), "letters are not icons");
        assert_eq!(ws.active_space().icon, "");
        assert!(ws.set_space_icon(space, "👨‍👩‍👧"));
        assert_eq!(ws.active_space().icon, "👨‍👩‍👧");
        let f = ws.add_folder("Work");
        assert!(ws.set_folder_icon(f, "📁"));
        assert_eq!(ws.active_space().folders[0].icon, "📁");
        assert!(!ws.set_folder_icon(FolderId(999), "📁"));
        assert!(!ws.set_space_icon(SpaceId(999), "📁"));
    }

    #[test]
    fn split_view_pairs_tabs_and_swaps_on_activate() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        let c = ws.open_tab(u("https://c.test"));
        assert!(!ws.set_split(c), "the active tab cannot be its own partner");
        assert!(!ws.set_split(TabId(999)));
        assert!(ws.set_split(a));
        assert_eq!(ws.split_tab(), Some(a));
        assert!(ws.activate_tab(a));
        assert_eq!((ws.active_tab().unwrap().id, ws.split_tab()), (a, Some(c)));
        assert!(ws.activate_tab(b));
        assert_eq!(
            ws.split_tab(),
            Some(c),
            "a third tab replaces the active pane only"
        );
        assert!(ws.close_tab(c));
        assert_eq!(ws.split_tab(), None);
        ws.clear_split();
        assert_eq!(ws.split_tab(), None);
    }

    #[test]
    fn restore_keeps_ids_unique() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let restored = Workspace::restore(ws.spaces().to_vec(), ws.active_space().id);
        let mut restored = restored;
        let b = restored.open_tab(u("https://b.test"));
        assert_ne!(a, b);
    }
}
