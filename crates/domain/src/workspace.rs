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
    /// The user chose the title: page title changes no longer replace it.
    pub renamed: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    /// Optional emoji shown instead of the default folder glyph.
    pub icon: String,
    pub collapsed: bool,
    /// Enclosing folder, `None` at the top level.
    pub parent: Option<FolderId>,
}

/// Deepest nesting allowed (a top-level folder is depth 1).
pub const MAX_FOLDER_DEPTH: usize = 6;

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

/// `id` plus every folder nested below it.
fn folder_subtree(folders: &[Folder], id: FolderId) -> Vec<FolderId> {
    if !folders.iter().any(|f| f.id == id) {
        return Vec::new();
    }
    let mut out = vec![id];
    let mut i = 0;
    while i < out.len() {
        let at = out[i];
        out.extend(
            folders
                .iter()
                .filter(|f| f.parent == Some(at))
                .map(|f| f.id),
        );
        i += 1;
    }
    out
}

/// Nesting level of a folder (1 at the top level); capped so corrupt data cannot loop.
fn folder_depth(folders: &[Folder], id: FolderId) -> usize {
    let mut depth = 0;
    let mut at = Some(id);
    while let Some(cur) = at {
        depth += 1;
        if depth > MAX_FOLDER_DEPTH * 2 {
            break;
        }
        at = folders.iter().find(|f| f.id == cur).and_then(|f| f.parent);
    }
    depth
}

/// Levels of the folder itself plus its deepest descendant chain (1 for a leaf).
fn folder_height(folders: &[Folder], id: FolderId) -> usize {
    1 + folders
        .iter()
        .filter(|f| f.parent == Some(id))
        .map(|f| folder_height(folders, f.id))
        .max()
        .unwrap_or(0)
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

/// How many recently shown tabs are remembered (session only) for Ctrl+Tab and for picking the
/// tab that takes over when the active one closes.
const RECENT_MAX: usize = 64;

#[derive(Debug, Clone)]
pub struct Workspace {
    spaces: Vec<Space>,
    active_space: SpaceId,
    next_id: u64,
    /// Tabs in the order they were last shown, most recent first, all spaces mixed. Not persisted.
    recent: Vec<TabId>,
}

/// The recency list is session state: two workspaces holding the same tabs are equal.
impl PartialEq for Workspace {
    fn eq(&self, other: &Self) -> bool {
        self.spaces == other.spaces
            && self.active_space == other.active_space
            && self.next_id == other.next_id
    }
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
            recent: Vec::new(),
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
        // The tab each space was showing counts as its most recent one, active space first.
        let mut recent: Vec<TabId> = spaces
            .iter()
            .filter(|s| s.id != active_space)
            .filter_map(|s| s.active_tab)
            .collect();
        if let Some(tab) = spaces
            .iter()
            .find(|s| s.id == active_space)
            .and_then(|s| s.active_tab)
        {
            recent.insert(0, tab);
        }
        Self {
            spaces,
            active_space,
            next_id,
            recent,
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

    /// Opens a tab and shows it. Like Arc, a new tab goes to the top of the unpinned list.
    pub fn open_tab(&mut self, url: Url) -> TabId {
        let id = TabId(self.alloc());
        let title = url.host_str().unwrap_or("New tab").to_owned();
        let space = self.active_space_mut();
        let at = space
            .tabs
            .iter()
            .position(|t| !t.pinned)
            .unwrap_or(space.tabs.len());
        space.tabs.insert(
            at,
            Tab {
                id,
                title,
                url,
                pinned: false,
                folder: None,
                renamed: false,
            },
        );
        space.active_tab = Some(id);
        self.touch(id);
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
            self.touch(id);
        }
        found
    }

    /// Closes a tab in the active space. When it was shown, its split partner takes over, else
    /// the tab seen just before it, else its neighbour in the unpinned list (none: start page).
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let Some(pos) = self.active_space().tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let next = self.successor(id, pos);
        self.recent.retain(|t| *t != id);
        let space = self.active_space_mut();
        space.tabs.remove(pos);
        Self::hand_over(space, id, next);
        true
    }

    /// Arc's ⌘W on a pinned or folder tab: the tab keeps its place in the sidebar but stops being
    /// shown; the next tab is chosen as for `close_tab`. Refused for ordinary tabs.
    pub fn release_tab(&mut self, id: TabId) -> bool {
        let Some(pos) = self
            .active_space()
            .tabs
            .iter()
            .position(|t| t.id == id && (t.pinned || t.folder.is_some()))
        else {
            return false;
        };
        let next = self.successor(id, pos);
        self.recent.retain(|t| *t != id);
        Self::hand_over(self.active_space_mut(), id, next);
        true
    }

    /// The tab shown just before the active one in this space (Ctrl+Tab).
    pub fn previous_tab(&self) -> Option<TabId> {
        let space = self.active_space();
        self.recent
            .iter()
            .copied()
            .find(|t| Some(*t) != space.active_tab && space.tabs.iter().any(|x| x.id == *t))
    }

    /// The tab `step` places below (positive) or above (negative) the active one, in sidebar
    /// order: quick-access tabs, tabs of unfolded folders, then the other tabs. Wraps around.
    pub fn sibling_tab(&self, step: isize) -> Option<TabId> {
        let space = self.active_space();
        let unfolded = |f: Option<FolderId>| {
            let mut at = f;
            let mut hops = 0;
            while let Some(id) = at {
                let Some(folder) = space.folders.iter().find(|x| x.id == id) else {
                    return false;
                };
                if folder.collapsed || hops > MAX_FOLDER_DEPTH * 2 {
                    return false;
                }
                at = folder.parent;
                hops += 1;
            }
            true
        };
        let order: Vec<TabId> = space
            .tabs
            .iter()
            .filter(|t| t.pinned)
            .chain(
                space
                    .tabs
                    .iter()
                    .filter(|t| !t.pinned && t.folder.is_some() && unfolded(t.folder)),
            )
            .chain(
                space
                    .tabs
                    .iter()
                    .filter(|t| !t.pinned && t.folder.is_none()),
            )
            .map(|t| t.id)
            .collect();
        if order.is_empty() {
            return None;
        }
        let len = order.len() as isize;
        let here = space
            .active_tab
            .and_then(|a| order.iter().position(|t| *t == a));
        let at = match here {
            Some(i) => (i as isize + step).rem_euclid(len),
            None if step < 0 => len - 1,
            None => 0,
        };
        order.get(at as usize).copied()
    }

    /// Remembers that `id` was just shown.
    fn touch(&mut self, id: TabId) {
        self.recent.retain(|t| *t != id);
        self.recent.insert(0, id);
        self.recent.truncate(RECENT_MAX);
    }

    /// The tab to show once `gone` (at `pos` in the active space) is no longer shown.
    fn successor(&self, gone: TabId, pos: usize) -> Option<TabId> {
        let space = self.active_space();
        if space.active_tab != Some(gone) {
            return space.active_tab;
        }
        let alive = |id: TabId| id != gone && space.tabs.iter().any(|t| t.id == id);
        if let Some(partner) = space.split.filter(|s| alive(*s)) {
            return Some(partner);
        }
        if let Some(seen) = self.recent.iter().copied().find(|t| alive(*t)) {
            return Some(seen);
        }
        // The loose tab that slides into its place, else the one above it.
        let loose = |t: &&Tab| t.id != gone && !t.pinned && t.folder.is_none();
        space
            .tabs
            .iter()
            .skip(pos)
            .find(loose)
            .or_else(|| {
                space.tabs[..pos.min(space.tabs.len())]
                    .iter()
                    .rev()
                    .find(loose)
            })
            .map(|t| t.id)
    }

    fn hand_over(space: &mut Space, gone: TabId, next: Option<TabId>) {
        if space.active_tab == Some(gone) {
            space.active_tab = next;
            // The partner now fills the window: the split is over.
            if space.split == next {
                space.split = None;
            }
        }
        if space.split == Some(gone) {
            space.split = None;
        }
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
        self.add_folder_in(name, None)
            .expect("a top-level folder is always allowed")
    }

    /// Creates a folder, inside `parent` when given (refused if it is unknown or too deep).
    pub fn add_folder_in(&mut self, name: &str, parent: Option<FolderId>) -> Option<FolderId> {
        if let Some(p) = parent {
            let space = self.active_space();
            if !space.folders.iter().any(|f| f.id == p)
                || folder_depth(&space.folders, p) >= MAX_FOLDER_DEPTH
            {
                return None;
            }
        }
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
            parent,
        });
        Some(id)
    }

    /// Files imported links as ordinary tabs in `space`, grouped in folders by path ("A/B" makes
    /// `B` a subfolder of `A`; blank: "Imported"). Links already open in the space are skipped; nothing is activated.
    /// Returns how many tabs were added.
    pub fn import_tabs(&mut self, space: SpaceId, links: Vec<(String, String, Url)>) -> usize {
        let Some(idx) = self.spaces.iter().position(|s| s.id == space) else {
            return 0;
        };
        let mut added = 0;
        for (folder, title, url) in links {
            if self.spaces[idx].tabs.iter().any(|t| t.url == url) {
                continue;
            }
            let mut parent: Option<FolderId> = None;
            let mut parts: Vec<&str> = folder
                .split('/')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();
            if parts.is_empty() {
                parts.push("Imported");
            }
            parts.truncate(MAX_FOLDER_DEPTH);
            for name in parts {
                let existing = self.spaces[idx]
                    .folders
                    .iter()
                    .find(|f| f.name == name && f.parent == parent)
                    .map(|f| f.id);
                parent = Some(existing.unwrap_or_else(|| {
                    let id = FolderId(self.alloc());
                    self.spaces[idx].folders.push(Folder {
                        id,
                        name: name.to_owned(),
                        icon: String::new(),
                        collapsed: true,
                        parent,
                    });
                    id
                }));
            }
            let folder_id = parent.expect("at least one folder level");
            let id = TabId(self.alloc());
            let title = match title.trim() {
                "" => url.host_str().unwrap_or("Link").to_owned(),
                t => t.to_owned(),
            };
            self.spaces[idx].tabs.push(Tab {
                id,
                title,
                url,
                pinned: false,
                folder: Some(folder_id),
                renamed: false,
            });
            added += 1;
        }
        added
    }

    /// Renames any space (not only the active one); blank names are refused.
    pub fn rename_space(&mut self, id: SpaceId, name: &str) -> bool {
        let name = name.trim();
        match self.spaces.iter_mut().find(|s| s.id == id) {
            Some(s) if !name.is_empty() => {
                s.name = name.to_owned();
                true
            }
            _ => false,
        }
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

    /// Moves a folder of the active space under `parent` (`None`: top level), before the sibling
    /// `before` or last. Refused for cycles, unknown ids and nesting beyond `MAX_FOLDER_DEPTH`.
    pub fn move_folder(
        &mut self,
        id: FolderId,
        parent: Option<FolderId>,
        before: Option<FolderId>,
    ) -> bool {
        let space = self.active_space_mut();
        let Some(from) = space.folders.iter().position(|f| f.id == id) else {
            return false;
        };
        if before == Some(id) || parent == Some(id) {
            return false;
        }
        if let Some(p) = parent {
            if !space.folders.iter().any(|f| f.id == p)
                || folder_subtree(&space.folders, id).contains(&p)
                || folder_depth(&space.folders, p) + folder_height(&space.folders, id)
                    > MAX_FOLDER_DEPTH
            {
                return false;
            }
        }
        let mut folder = space.folders.remove(from);
        folder.parent = parent;
        let to = before
            .and_then(|b| space.folders.iter().position(|f| f.id == b))
            .unwrap_or(space.folders.len());
        space.folders.insert(to, folder);
        true
    }

    /// The folder and all its subfolders, in the active space.
    pub fn folder_tree(&self, id: FolderId) -> Vec<FolderId> {
        folder_subtree(&self.active_space().folders, id)
    }

    /// Removes a folder and its subfolders; their tabs stay open, back in the ungrouped list.
    pub fn delete_folder(&mut self, id: FolderId) -> bool {
        let space = self.active_space_mut();
        let doomed = folder_subtree(&space.folders, id);
        let before = space.folders.len();
        space.folders.retain(|f| !doomed.contains(&f.id));
        for tab in space
            .tabs
            .iter_mut()
            .filter(|t| t.folder.is_some_and(|f| doomed.contains(&f)))
        {
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
                if !tab.renamed {
                    tab.title = title;
                }
            }
        }
    }

    /// Gives a tab a title of the user's choosing; a blank name hands the title back to the page.
    pub fn rename_tab(&mut self, id: TabId, name: &str) -> bool {
        let name = name.trim();
        let tab = self
            .spaces
            .iter_mut()
            .flat_map(|s| s.tabs.iter_mut())
            .find(|t| t.id == id);
        let Some(tab) = tab else { return false };
        if name.is_empty() {
            tab.renamed = false;
            tab.title = tab.url.host_str().unwrap_or("New tab").to_owned();
        } else {
            tab.renamed = true;
            name.clone_into(&mut tab.title);
        }
        true
    }

    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Changes the identity color of a space; only `#rrggbb` is accepted.
    pub fn set_space_color(&mut self, id: SpaceId, color: &str) -> bool {
        let valid = color.len() == 7
            && color.starts_with('#')
            && color[1..].chars().all(|c| c.is_ascii_hexdigit());
        match self.spaces.iter_mut().find(|s| s.id == id) {
            Some(space) if valid => {
                space.color = color.to_ascii_lowercase();
                true
            }
            _ => false,
        }
    }

    /// Moves a tab of the active space to another space: at the top of its unpinned list, or at
    /// the end of its quick access tiles when pinned, never inside a folder. When it was shown,
    /// the next tab is chosen as for `close_tab`.
    pub fn move_tab_to_space(&mut self, id: TabId, to: SpaceId) -> bool {
        if to == self.active_space || !self.spaces.iter().any(|s| s.id == to) {
            return false;
        }
        let Some(pos) = self.active_space().tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let next = self.successor(id, pos);
        self.recent.retain(|t| *t != id);
        let space = self.active_space_mut();
        let mut tab = space.tabs.remove(pos);
        Self::hand_over(space, id, next);
        tab.folder = None;
        let target = self
            .spaces
            .iter_mut()
            .find(|s| s.id == to)
            .expect("target space checked above");
        let at = target
            .tabs
            .iter()
            .position(|t| !t.pinned)
            .unwrap_or(target.tabs.len());
        target.tabs.insert(at, tab);
        true
    }

    /// Everything a space holds, taken before it is deleted so it can be put back.
    pub fn snapshot_space(&self, id: SpaceId) -> Option<Removed> {
        let at = self.spaces.iter().position(|s| s.id == id)?;
        Some(Removed::Space {
            space: self.spaces[at].clone(),
            at,
        })
    }

    /// A folder of the active space, its subfolders and their tabs, taken before deletion.
    pub fn snapshot_folder(&self, id: FolderId) -> Option<Removed> {
        let space = self.active_space();
        let tree = folder_subtree(&space.folders, id);
        if tree.is_empty() {
            return None;
        }
        let pick = |f: &Option<FolderId>| f.is_some_and(|f| tree.contains(&f));
        Some(Removed::Folder {
            space: space.id,
            folders: space
                .folders
                .iter()
                .enumerate()
                .filter(|(_, f)| tree.contains(&f.id))
                .map(|(i, f)| (i, f.clone()))
                .collect(),
            tabs: space
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, t)| pick(&t.folder))
                .map(|(i, t)| (i, t.clone()))
                .collect(),
        })
    }

    /// Puts back a deleted space or folder (undo), at its former place. Refused when it is
    /// still there or its space is gone; tabs that exist again are left alone.
    pub fn undo_removal(&mut self, removed: Removed) -> bool {
        match removed {
            Removed::Space { space, at } => {
                if self.spaces.iter().any(|s| s.id == space.id) {
                    return false;
                }
                let at = at.min(self.spaces.len());
                self.spaces.insert(at, space);
                true
            }
            Removed::Folder {
                space,
                folders,
                tabs,
            } => {
                let Some(target) = self.spaces.iter_mut().find(|s| s.id == space) else {
                    return false;
                };
                if folders
                    .iter()
                    .any(|(_, f)| target.folders.iter().any(|x| x.id == f.id))
                {
                    return false;
                }
                for (i, folder) in folders {
                    let at = i.min(target.folders.len());
                    target.folders.insert(at, folder);
                }
                for (i, tab) in tabs {
                    if !target.tabs.iter().any(|t| t.id == tab.id) {
                        let at = i.min(target.tabs.len());
                        target.tabs.insert(at, tab);
                    }
                }
                true
            }
        }
    }
}

/// A folder suggested by `Workspace::tidy_suggestion`: the loose tabs of one site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyGroup {
    pub name: String,
    pub tabs: Vec<TabId>,
}

/// What `Workspace::tidy` did, to undo it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tidied {
    created: Vec<FolderId>,
    moved: Vec<TabId>,
}

impl Tidied {
    pub fn tabs(&self) -> usize {
        self.moved.len()
    }
}

/// The site of a host, for grouping: `www.` dropped and subdomains folded into the registrable
/// part (`docs.github.com` → `github.com`, `news.bbc.co.uk` → `bbc.co.uk`).
pub fn site_of_host(host: &str) -> String {
    let host = host.trim_start_matches("www.").to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    // A short second-level label under a country code is part of the suffix (co.uk, com.au).
    let keep = if n >= 3 && labels[n - 1].len() == 2 && labels[n - 2].len() <= 3 { 3 } else { 2 };
    labels[n.saturating_sub(keep)..].join(".")
}

impl Workspace {
    /// Loose tabs of the active space grouped by site: every site with `min` tabs or more is a
    /// folder to make (or to fill, when a folder of that name exists), biggest first.
    pub fn tidy_suggestion(&self, min: usize) -> Vec<TidyGroup> {
        let mut groups: Vec<TidyGroup> = Vec::new();
        for tab in self.active_space().tabs.iter().filter(|t| !t.pinned && t.folder.is_none()) {
            let Some(host) = tab.url.host_str() else { continue };
            let name = site_of_host(host);
            match groups.iter_mut().find(|g| g.name == name) {
                Some(g) => g.tabs.push(tab.id),
                None => groups.push(TidyGroup { name, tabs: vec![tab.id] }),
            }
        }
        groups.retain(|g| g.tabs.len() >= min.max(2));
        groups.sort_by(|a, b| b.tabs.len().cmp(&a.tabs.len()).then_with(|| a.name.cmp(&b.name)));
        groups
    }

    /// Files each group's tabs into a folder named after it (an existing top-level folder of
    /// that name is reused, else one is made).
    pub fn tidy(&mut self, groups: &[TidyGroup]) -> Tidied {
        let mut done = Tidied::default();
        for group in groups {
            let existing = self
                .active_space()
                .folders
                .iter()
                .find(|f| f.parent.is_none() && f.name.eq_ignore_ascii_case(&group.name))
                .map(|f| f.id);
            let folder = match existing {
                Some(id) => id,
                None => {
                    let id = self.add_folder(&group.name);
                    done.created.push(id);
                    id
                }
            };
            for &tab in &group.tabs {
                if self.move_tab(tab, None, Some(folder), false) {
                    done.moved.push(tab);
                }
            }
        }
        done
    }

    /// Puts the tabs back out of their folders and removes the folders `tidy` made.
    pub fn untidy(&mut self, done: &Tidied) {
        for &tab in &done.moved {
            self.move_tab(tab, None, None, false);
        }
        for &folder in &done.created {
            self.delete_folder(folder);
        }
    }
}

/// A deleted space or folder with everything it held (see `Workspace::undo_removal`).
#[derive(Debug, Clone, PartialEq)]
pub enum Removed {
    Space {
        space: Space,
        at: usize,
    },
    Folder {
        space: SpaceId,
        folders: Vec<(usize, Folder)>,
        tabs: Vec<(usize, Tab)>,
    },
}

impl Removed {
    /// The tabs that come back with it.
    pub fn tabs(&self) -> Vec<&Tab> {
        match self {
            Removed::Space { space, .. } => space.tabs.iter().collect(),
            Removed::Folder { tabs, .. } => tabs.iter().map(|(_, t)| t).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn sites_fold_subdomains_and_country_suffixes() {
        assert_eq!(site_of_host("www.GitHub.com"), "github.com");
        assert_eq!(site_of_host("docs.github.com"), "github.com");
        assert_eq!(site_of_host("news.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(site_of_host("localhost"), "localhost");
    }

    #[test]
    fn tidying_files_the_loose_tabs_of_a_site_and_can_be_undone() {
        let mut ws = Workspace::new();
        let gh: Vec<TabId> = ["https://github.com/a", "https://docs.github.com/b", "https://github.com/c"]
            .iter()
            .map(|raw| ws.open_tab(u(raw)))
            .collect();
        let lone = ws.open_tab(u("https://example.com/"));
        let pinned = ws.open_tab(u("https://github.com/pinned"));
        ws.toggle_pin(pinned);
        let groups = ws.tidy_suggestion(3);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "github.com");
        assert_eq!(groups[0].tabs.len(), 3, "pinned and other sites are left alone");
        let done = ws.tidy(&groups);
        assert_eq!(done.tabs(), 3);
        let folder = ws.active_space().folders[0].id;
        assert!(gh.iter().all(|id| ws.active_space().tabs.iter().any(|t| t.id == *id && t.folder == Some(folder))));
        assert!(ws.tidy_suggestion(3).is_empty());
        // A folder of the same name is reused the next time.
        ws.open_tab(u("https://github.com/d"));
        ws.open_tab(u("https://github.com/e"));
        ws.open_tab(u("https://github.com/f"));
        let again = ws.tidy(&ws.tidy_suggestion(3));
        assert_eq!(ws.active_space().folders.len(), 1);
        ws.untidy(&again);
        ws.untidy(&done);
        assert!(ws.active_space().folders.is_empty());
        assert!(ws.active_space().tabs.iter().all(|t| t.folder.is_none()));
        assert!(ws.active_space().tabs.iter().any(|t| t.id == lone));
    }

    #[test]
    fn a_space_color_must_be_a_hex_color() {
        let mut ws = Workspace::new();
        let id = ws.active_space().id;
        assert!(ws.set_space_color(id, "#FF5C8A"));
        assert_eq!(ws.active_space().color, "#ff5c8a");
        assert!(!ws.set_space_color(id, "red"));
        assert!(!ws.set_space_color(id, "#12345g"));
        assert!(!ws.set_space_color(SpaceId(999), "#000000"));
        assert_eq!(ws.active_space().color, "#ff5c8a");
    }

    #[test]
    fn a_tab_moves_to_another_space_out_of_its_folder_and_the_previous_tab_takes_over() {
        let mut ws = Workspace::new();
        let first = ws.open_tab(u("https://a.test/"));
        let moved = ws.open_tab(u("https://b.test/"));
        let folder = ws.add_folder("F");
        assert!(ws.move_tab(moved, None, Some(folder), false));
        ws.activate_tab(moved);
        let other = ws.add_space("Work", "#3aa0ff");
        assert!(ws.move_tab_to_space(moved, other));
        assert_eq!(ws.active_tab().map(|t| t.id), Some(first));
        assert!(!ws.active_space().tabs.iter().any(|t| t.id == moved));
        let target = ws.spaces().iter().find(|s| s.id == other).unwrap();
        assert_eq!(target.tabs[0].id, moved);
        assert_eq!(target.tabs[0].folder, None);
        // Not to the space it is in, nor to an unknown one.
        assert!(!ws.move_tab_to_space(first, ws.active_space().id));
        assert!(!ws.move_tab_to_space(first, SpaceId(999)));
    }

    #[test]
    fn a_deleted_space_comes_back_with_its_tabs_at_its_place() {
        let mut ws = Workspace::new();
        let other = ws.add_space("Work", "#3aa0ff");
        ws.switch_space(other);
        let tab = ws.open_tab(u("https://a.test/"));
        let snap = ws.snapshot_space(other).unwrap();
        assert_eq!(snap.tabs().len(), 1);
        ws.delete_space(other).unwrap();
        assert_eq!(ws.spaces().len(), 1);
        assert!(ws.undo_removal(snap.clone()));
        assert_eq!(ws.spaces()[1].id, other);
        assert_eq!(ws.spaces()[1].tabs[0].id, tab);
        // A second undo does nothing.
        assert!(!ws.undo_removal(snap));
    }

    #[test]
    fn a_deleted_folder_comes_back_with_its_subfolders_and_tabs() {
        let mut ws = Workspace::new();
        let loose = ws.open_tab(u("https://loose.test/"));
        let inner = ws.open_tab(u("https://inner.test/"));
        let deep = ws.open_tab(u("https://deep.test/"));
        let top = ws.add_folder("Top");
        let sub = ws.add_folder_in("Sub", Some(top)).unwrap();
        ws.move_tab(inner, None, Some(top), false);
        ws.move_tab(deep, None, Some(sub), false);
        let before = ws.active_space().clone();
        let snap = ws.snapshot_folder(top).unwrap();
        assert_eq!(snap.tabs().len(), 2);
        // What the app does: close the tabs inside, then delete the folder.
        ws.close_tab(inner);
        ws.close_tab(deep);
        ws.delete_folder(top);
        assert!(ws.undo_removal(snap));
        let after = ws.active_space();
        assert_eq!(after.folders, before.folders);
        let ids = |s: &Space| s.tabs.iter().map(|t| (t.id, t.folder)).collect::<Vec<_>>();
        assert_eq!(ids(after), ids(&before));
        assert!(after.tabs.iter().any(|t| t.id == loose));
    }

    #[test]
    fn renamed_tab_keeps_its_title_until_blanked() {
        let mut ws = Workspace::new();
        let id = ws.open_tab(u("https://example.com/"));
        assert!(ws.rename_tab(id, "  Mine "));
        ws.update_page(id, None, Some("Page".into()));
        assert_eq!(ws.active_space().tabs[0].title, "Mine");
        assert!(ws.rename_tab(id, ""));
        ws.update_page(id, None, Some("Page".into()));
        assert_eq!(ws.active_space().tabs[0].title, "Page");
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
    fn imported_links_become_tabs_in_named_folders_without_duplicates() {
        let mut ws = Workspace::new();
        let sp = ws.active_space().id;
        let links = vec![
            ("Work".to_owned(), "A".to_owned(), u("https://a.test")),
            ("Work".to_owned(), String::new(), u("https://b.test")),
            (String::new(), "A again".to_owned(), u("https://a.test")),
        ];
        assert_eq!(ws.import_tabs(sp, links), 2);
        let s = ws.active_space();
        assert_eq!(s.folders.len(), 1);
        assert!(s.tabs.iter().all(|t| t.folder == Some(s.folders[0].id)));
        assert_eq!(s.tabs[1].title, "b.test");
        assert_eq!(s.active_tab, None);
    }

    #[test]
    fn folders_can_be_reordered() {
        let mut ws = Workspace::new();
        let (a, b, c) = (ws.add_folder("A"), ws.add_folder("B"), ws.add_folder("C"));
        let names = |ws: &Workspace| -> String {
            ws.active_space()
                .folders
                .iter()
                .map(|f| f.name.clone())
                .collect()
        };
        assert!(ws.move_folder(c, None, Some(a)));
        assert_eq!(names(&ws), "CAB");
        assert!(ws.move_folder(c, None, None));
        assert_eq!(names(&ws), "ABC");
        assert!(!ws.move_folder(b, None, Some(b)));
        assert!(!ws.move_folder(FolderId(999), None, None));
    }

    #[test]
    fn folders_nest_without_cycles_and_delete_recursively() {
        let mut ws = Workspace::new();
        let a = ws.add_folder("A");
        let b = ws.add_folder_in("B", Some(a)).unwrap();
        let c = ws.add_folder_in("C", Some(b)).unwrap();
        assert_eq!(ws.folder_tree(a), vec![a, b, c]);
        assert!(!ws.move_folder(a, Some(c), None), "cycle");
        assert!(!ws.move_folder(a, Some(a), None));
        assert!(ws.add_folder_in("X", Some(FolderId(999))).is_none());
        assert!(ws.move_folder(c, None, None));
        assert_eq!(ws.folder_tree(a), vec![a, b]);
        assert!(ws.move_folder(c, Some(a), Some(b)));
        let t = ws.open_tab(u("https://deep.test"));
        ws.move_tab(t, None, Some(c), false);
        assert!(ws.delete_folder(a));
        assert!(ws.active_space().folders.is_empty());
        assert_eq!(ws.active_space().tabs[0].folder, None);
    }

    #[test]
    fn folder_depth_is_capped() {
        let mut ws = Workspace::new();
        let mut parent = None;
        for i in 0..MAX_FOLDER_DEPTH {
            parent = Some(ws.add_folder_in(&format!("L{i}"), parent).unwrap());
        }
        assert!(ws.add_folder_in("too deep", parent).is_none());
    }

    #[test]
    fn imported_paths_keep_their_hierarchy() {
        let mut ws = Workspace::new();
        let sp = ws.active_space().id;
        let links = vec![
            (
                "dossier_1/dossier_2".to_owned(),
                "A".to_owned(),
                u("https://a.test"),
            ),
            ("dossier_1".to_owned(), "B".to_owned(), u("https://b.test")),
            (
                "dossier_1/dossier_2".to_owned(),
                "C".to_owned(),
                u("https://c.test"),
            ),
        ];
        assert_eq!(ws.import_tabs(sp, links), 3);
        let s = ws.active_space();
        assert_eq!(s.folders.len(), 2);
        let d1 = s.folders.iter().find(|f| f.name == "dossier_1").unwrap();
        let d2 = s.folders.iter().find(|f| f.name == "dossier_2").unwrap();
        assert_eq!(d1.parent, None);
        assert_eq!(d2.parent, Some(d1.id));
        assert_eq!(s.tabs.iter().filter(|t| t.folder == Some(d2.id)).count(), 2);
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
    fn closing_returns_to_the_tab_seen_before() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        let c = ws.open_tab(u("https://c.test"));
        ws.activate_tab(a);
        ws.activate_tab(c);
        assert!(ws.close_tab(c));
        assert_eq!(
            ws.active_tab().unwrap().id,
            a,
            "last seen, not the neighbour"
        );
        assert!(ws.close_tab(a));
        assert_eq!(ws.active_tab().unwrap().id, b);
        assert!(ws.close_tab(b));
        assert_eq!(ws.active_tab(), None);
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_one() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        assert!(ws.close_tab(a));
        assert_eq!(ws.active_tab().unwrap().id, b);
    }

    #[test]
    fn closing_without_history_falls_back_to_the_loose_neighbour() {
        let mut ws = Workspace::new();
        let sp = ws.active_space().id;
        ws.import_tabs(sp, vec![(String::new(), "F".into(), u("https://f.test"))]);
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        ws.recent.clear();
        // Order: b, a (newest first), then the imported tab, which sits in a folder.
        assert!(ws.close_tab(b));
        assert_eq!(ws.active_tab().unwrap().id, a);
    }

    #[test]
    fn closing_one_pane_of_a_split_shows_the_other_alone() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        let c = ws.open_tab(u("https://c.test"));
        ws.activate_tab(b);
        ws.activate_tab(c);
        assert!(ws.set_split(a));
        assert!(ws.close_tab(c));
        assert_eq!(ws.active_tab().unwrap().id, a);
        assert_eq!(ws.split_tab(), None);
        assert!(ws.activate_tab(b));
        assert_eq!(ws.split_tab(), None, "the old split does not come back");
    }

    #[test]
    fn releasing_a_pinned_tab_keeps_it_but_shows_another() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let p = ws.open_tab(u("https://p.test"));
        ws.toggle_pin(p);
        assert!(!ws.release_tab(a), "ordinary tabs are closed, not released");
        assert!(ws.release_tab(p));
        assert_eq!(ws.active_tab().unwrap().id, a);
        assert!(ws.active_space().tabs.iter().any(|t| t.id == p && t.pinned));
    }

    #[test]
    fn ctrl_tab_goes_back_and_forth_between_the_last_two_tabs() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        ws.open_tab(u("https://b.test"));
        let c = ws.open_tab(u("https://c.test"));
        ws.activate_tab(a);
        assert_eq!(ws.previous_tab(), Some(c));
        ws.activate_tab(c);
        assert_eq!(ws.previous_tab(), Some(a));
        let other = ws.add_space("Other", "#fff");
        ws.switch_space(other);
        assert_eq!(ws.previous_tab(), None, "spaces are separate");
    }

    #[test]
    fn sibling_tabs_follow_the_sidebar_order_and_wrap() {
        let mut ws = Workspace::new();
        let a = ws.open_tab(u("https://a.test"));
        let b = ws.open_tab(u("https://b.test"));
        let p = ws.open_tab(u("https://p.test"));
        ws.toggle_pin(p);
        let f = ws.add_folder("F");
        let inner = ws.open_tab(u("https://f.test"));
        ws.move_tab(inner, None, Some(f), false);
        // Sidebar: p | inner | b, a
        ws.activate_tab(p);
        assert_eq!(ws.sibling_tab(1), Some(inner));
        ws.toggle_folder(f);
        assert_eq!(ws.sibling_tab(1), Some(b), "folded tabs are skipped");
        ws.activate_tab(a);
        assert_eq!(ws.sibling_tab(1), Some(p), "wraps around");
        assert_eq!(ws.sibling_tab(-1), Some(b));
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
        let order = |ws: &Workspace| {
            ws.active_space()
                .tabs
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&ws), vec![c, b, a], "new tabs go on top");
        assert!(ws.move_tab(c, Some(a), None, false));
        assert_eq!(order(&ws), vec![b, c, a]);

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
    fn spaces_can_be_renamed_even_when_not_active() {
        let mut ws = Workspace::default();
        let other = ws.add_space("Work", "#fff");
        let first = ws.spaces()[0].id;
        assert!(ws.rename_space(first, "  Home  "));
        assert_eq!(ws.spaces()[0].name, "Home");
        assert!(!ws.rename_space(other, "   "));
        assert!(!ws.rename_space(SpaceId(9999), "x"));
        assert_eq!(ws.spaces()[1].name, "Work");
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
