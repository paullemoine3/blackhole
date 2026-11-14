//! Easels: free-form boards where notes and clips of web pages are laid out.
//! A clip keeps a link back to the page it was captured from (http/https only).

use url::Url;

pub const MAX_EASELS: usize = 100;
pub const MAX_ITEMS: usize = 500;
pub const MAX_NAME: usize = 80;
pub const MAX_TEXT: usize = 20_000;
pub const MAX_TITLE: usize = 300;
pub const MAX_SELECTOR: usize = 500;
/// Height of a live card's title bar (30) and footer (24) in `ui/easel.html`: a live card adds
/// it to the captured area's height, so its slot has the proportions of the area.
pub const BAR_HEIGHT: f64 = 54.0;
/// A page narrower or wider than this was not measured by a real window.
const VIEWPORT: (f64, f64) = (200.0, 8000.0);
/// How far the live view may be zoomed to show the page at the width it was picked in.
const LIVE_ZOOM: (f64, f64) = (0.25, 3.0);
/// Board coordinates are clamped so a bad value can never push an item out of reach.
pub const COORD_LIMIT: f64 = 100_000.0;
pub const MIN_SIZE: (f64, f64) = (120.0, 80.0);
pub const MAX_SIZE: (f64, f64) = (1200.0, 1200.0);
/// Live cards may be as tall as the largest card allows.
const MIN_SIZE_LIVE: (f64, f64) = (120.0, MAX_SIZE.1);
pub const DEFAULT_SIZE: (f64, f64) = (240.0, 160.0);
pub const COLORS: [&str; 6] = [
    "#2b2a3a", "#7c5cff", "#2e8b6e", "#c9822b", "#c2455b", "#2f6fb5",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Note,
    Clip,
    /// A region of a page (CSS selector) shown live inside the card.
    Live,
}

impl ItemKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Clip => "clip",
            Self::Live => "live",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "note" => Some(Self::Note),
            "clip" => Some(Self::Clip),
            "live" => Some(Self::Live),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub kind: ItemKind,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    pub title: String,
    pub url: Option<String>,
    /// CSS selector of the captured region (live cards only, empty otherwise).
    pub selector: String,
    pub color: String,
    /// Width of the page when the region was picked (live cards; 0: not known). The live view
    /// lays the page out at this width again, so the region keeps its shape.
    pub viewport: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Easel {
    pub id: u64,
    pub name: String,
    /// Sidebar folder (id of a workspace folder) the easel is filed in, like a tab.
    pub folder: Option<u64>,
    /// Space the easel belongs to when it is loose (`None`: the first space, for old data).
    pub space: Option<u64>,
    pub items: Vec<Item>,
}

impl Easel {
    fn next_item_id(&self) -> u64 {
        self.items.iter().map(|i| i.id).max().unwrap_or(0) + 1
    }

    /// Cascading grid slot for items added without an explicit position.
    fn free_slot(&self) -> (f64, f64) {
        let n = self.items.len();
        (
            40.0 + (n % 4) as f64 * (DEFAULT_SIZE.0 + 24.0),
            40.0 + (n / 4) as f64 * (DEFAULT_SIZE.1 + 24.0),
        )
    }

    fn item_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Easels {
    easels: Vec<Easel>,
}

fn clean(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn coord(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(-COORD_LIMIT, COORD_LIMIT)
    } else {
        0.0
    }
}

fn viewport(width: f64) -> f64 {
    if width.is_finite() && width >= VIEWPORT.0 && width <= VIEWPORT.1 {
        width
    } else {
        0.0
    }
}

/// The zoom of a live view `slot` pixels wide, for a region picked in a page `viewport` pixels
/// wide: at this zoom the page is laid out at the width it had, and the region has the shape
/// the card was sized after. 1 when the width is not known (cards made before it was kept).
pub fn live_zoom(viewport: f64, slot: f64) -> f64 {
    if viewport > 0.0 && slot.is_finite() && slot > 0.0 {
        (slot / viewport).clamp(LIVE_ZOOM.0, LIVE_ZOOM.1)
    } else {
        1.0
    }
}

fn size(v: f64, (lo, hi): (f64, f64)) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        lo
    }
}

/// Selectors are only ever passed to `querySelector` as a string; this keeps them printable and bounded.
fn clean_selector(raw: &str) -> String {
    let s = raw.trim();
    if s.chars().any(char::is_control) {
        return String::new();
    }
    clean(s, MAX_SELECTOR)
}

/// Only web pages can be linked: no `javascript:`, `file:` or app schemes.
pub fn clip_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then(|| url.to_string())
}

impl Easels {
    /// Rebuilds the collection from stored data, dropping what breaks the invariants.
    pub fn from_stored(easels: Vec<Easel>) -> Self {
        let mut out = Self::default();
        for mut easel in easels.into_iter().take(MAX_EASELS) {
            if out.get(easel.id).is_some() {
                continue;
            }
            easel.name = clean(easel.name.trim(), MAX_NAME);
            easel.items.truncate(MAX_ITEMS);
            for item in &mut easel.items {
                item.x = coord(item.x);
                item.y = coord(item.y);
                item.viewport = viewport(item.viewport);
                item.w = size(item.w, (MIN_SIZE.0, MAX_SIZE.0));
                item.h = size(item.h, (MIN_SIZE.1, MAX_SIZE.1));
                item.text = clean(&item.text, MAX_TEXT);
                item.title = clean(&item.title, MAX_TITLE);
                item.url = item.url.as_deref().and_then(clip_url);
                item.selector = if item.kind == ItemKind::Live {
                    clean_selector(&item.selector)
                } else {
                    String::new()
                };
                if !COLORS.contains(&item.color.as_str()) {
                    item.color = COLORS[0].to_owned();
                }
            }
            easel.items.retain(|i| match i.kind {
                ItemKind::Note => true,
                ItemKind::Clip => i.url.is_some(),
                ItemKind::Live => i.url.is_some() && !i.selector.is_empty(),
            });
            out.easels.push(easel);
        }
        out
    }

    pub fn all(&self) -> &[Easel] {
        &self.easels
    }

    pub fn get(&self, id: u64) -> Option<&Easel> {
        self.easels.iter().find(|e| e.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Easel> {
        self.easels.iter_mut().find(|e| e.id == id)
    }

    /// Creates an easel; a blank name becomes "Easel N". `None` when the limit is reached.
    pub fn create(&mut self, name: &str) -> Option<u64> {
        if self.easels.len() >= MAX_EASELS {
            return None;
        }
        let id = self.easels.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let name = clean(name.trim(), MAX_NAME);
        let name = if name.is_empty() {
            format!("Easel {}", self.easels.len() + 1)
        } else {
            name
        };
        self.easels.push(Easel {
            id,
            name,
            folder: None,
            space: None,
            items: Vec::new(),
        });
        Some(id)
    }

    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let name = clean(name.trim(), MAX_NAME);
        match self.get_mut(id) {
            Some(e) if !name.is_empty() => {
                e.name = name;
                true
            }
            _ => false,
        }
    }

    /// Files the easel in a sidebar folder (`None`: loose).
    pub fn set_folder(&mut self, id: u64, folder: Option<u64>) -> bool {
        match self.get_mut(id) {
            Some(e) if e.folder != folder => {
                e.folder = folder;
                true
            }
            _ => false,
        }
    }

    pub fn set_space(&mut self, id: u64, space: Option<u64>) -> bool {
        match self.get_mut(id) {
            Some(e) if e.space != space => {
                e.space = space;
                true
            }
            _ => false,
        }
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.easels.len();
        self.easels.retain(|e| e.id != id);
        self.easels.len() != before
    }

    pub fn add_note(&mut self, easel: u64, at: Option<(f64, f64)>) -> Option<u64> {
        let e = self.get_mut(easel)?;
        if e.items.len() >= MAX_ITEMS {
            return None;
        }
        let (x, y) = at.map_or_else(|| e.free_slot(), |(x, y)| (coord(x), coord(y)));
        let id = e.next_item_id();
        e.items.push(Item {
            id,
            kind: ItemKind::Note,
            x,
            y,
            w: DEFAULT_SIZE.0,
            h: DEFAULT_SIZE.1,
            text: String::new(),
            title: String::new(),
            url: None,
            selector: String::new(),
            color: COLORS[0].to_owned(),
            viewport: 0.0,
        });
        Some(id)
    }

    /// Adds a note that already holds `text` (a table of the comparator, a text received from
    /// another device): the text is only ever kept as text.
    pub fn add_text(&mut self, easel: u64, text: &str) -> Result<u64, &'static str> {
        let text = clean(text.trim(), MAX_TEXT);
        if text.is_empty() {
            return Err("msg.easel_nothing_selected");
        }
        let id = self.add_note(easel, None).ok_or("msg.easel_full")?;
        self.with_item(easel, id, |i| i.text = text);
        Ok(id)
    }

    /// Adds a clip of a page: its address, title and the text that was selected (may be empty).
    pub fn capture(
        &mut self,
        easel: u64,
        url: &str,
        title: &str,
        snippet: &str,
    ) -> Result<u64, &'static str> {
        let url = clip_url(url).ok_or("msg.easel_only_web")?;
        let e = self.get_mut(easel).ok_or("msg.easel_unknown")?;
        if e.items.len() >= MAX_ITEMS {
            return Err("msg.easel_full");
        }
        let (x, y) = e.free_slot();
        let id = e.next_item_id();
        let title = clean(title.trim(), MAX_TITLE);
        e.items.push(Item {
            id,
            kind: ItemKind::Clip,
            x,
            y,
            w: DEFAULT_SIZE.0,
            h: DEFAULT_SIZE.1,
            text: clean(snippet.trim(), MAX_TEXT),
            title: if title.is_empty() { url.clone() } else { title },
            url: Some(url),
            selector: String::new(),
            color: COLORS[1].to_owned(),
            viewport: 0.0,
        });
        Ok(id)
    }

    /// Adds a live card for the region `selector` of a page, sized like that region
    /// (`width` x `height` in page pixels) and scaled to a comfortable card width.
    pub fn capture_live(
        &mut self,
        easel: u64,
        url: &str,
        title: &str,
        selector: &str,
        (width, height): (f64, f64),
    ) -> Result<u64, &'static str> {
        let url = clip_url(url).ok_or("msg.easel_only_web")?;
        let selector = clean_selector(selector);
        if selector.is_empty() {
            return Err("msg.easel_nothing_selected");
        }
        let e = self.get_mut(easel).ok_or("msg.easel_unknown")?;
        if e.items.len() >= MAX_ITEMS {
            return Err("msg.easel_full");
        }
        let ratio = if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
            height / width
        } else {
            0.6
        };
        let w = size(if width.is_finite() { width } else { 0.0 }, (240.0, 640.0));
        let h = size(w * ratio + BAR_HEIGHT, MIN_SIZE_LIVE);
        let (x, y) = e.free_slot();
        let id = e.next_item_id();
        let title = clean(title.trim(), MAX_TITLE);
        e.items.push(Item {
            id,
            kind: ItemKind::Live,
            x,
            y,
            w,
            h,
            text: String::new(),
            title: if title.is_empty() { url.clone() } else { title },
            url: Some(url),
            selector,
            color: COLORS[3].to_owned(),
            viewport: 0.0,
        });
        Ok(id)
    }

    fn with_item(&mut self, easel: u64, item: u64, f: impl FnOnce(&mut Item)) -> bool {
        match self.get_mut(easel).and_then(|e| e.item_mut(item)) {
            Some(i) => {
                f(i);
                true
            }
            None => false,
        }
    }

    pub fn set_text(&mut self, easel: u64, item: u64, text: &str) -> bool {
        self.with_item(easel, item, |i| i.text = clean(text, MAX_TEXT))
    }

    pub fn move_item(&mut self, easel: u64, item: u64, x: f64, y: f64) -> bool {
        self.with_item(easel, item, |i| {
            i.x = coord(x);
            i.y = coord(y);
        })
    }

    pub fn resize_item(&mut self, easel: u64, item: u64, w: f64, h: f64) -> bool {
        self.with_item(easel, item, |i| {
            i.w = size(w, (MIN_SIZE.0, MAX_SIZE.0));
            i.h = size(h, (MIN_SIZE.1, MAX_SIZE.1));
        })
    }

    /// The width of the page a live card was picked in (reported by the page: checked).
    pub fn set_viewport(&mut self, easel: u64, item: u64, width: f64) -> bool {
        self.with_item(easel, item, |i| i.viewport = viewport(width))
    }

    /// Colours come from a fixed palette: the value is never trusted as CSS.
    pub fn set_color(&mut self, easel: u64, item: u64, color: &str) -> bool {
        COLORS.contains(&color) && self.with_item(easel, item, |i| i.color = color.to_owned())
    }

    pub fn remove_item(&mut self, easel: u64, item: u64) -> bool {
        self.get_mut(easel).is_some_and(|e| {
            let before = e.items.len();
            e.items.retain(|i| i.id != item);
            e.items.len() != before
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> (Easels, u64) {
        let mut easels = Easels::default();
        let id = easels.create("").unwrap();
        (easels, id)
    }

    #[test]
    fn blank_names_are_numbered_and_names_are_bounded() {
        let mut easels = Easels::default();
        let a = easels.create("  ").unwrap();
        let b = easels.create(&"x".repeat(500)).unwrap();
        assert_eq!(easels.get(a).unwrap().name, "Easel 1");
        assert_eq!(easels.get(b).unwrap().name.chars().count(), MAX_NAME);
        assert!(!easels.rename(a, "   "));
        assert!(easels.rename(a, " Ideas "));
        assert_eq!(easels.get(a).unwrap().name, "Ideas");
    }

    #[test]
    fn a_note_can_arrive_with_its_text() {
        let (mut easels, id) = board();
        let note = easels.add_text(id, "  Price | 10 €
Rating | 4.5  ").unwrap();
        let item = &easels.get(id).unwrap().items[0];
        assert_eq!((item.id, item.kind), (note, ItemKind::Note));
        assert_eq!(item.text, "Price | 10 €
Rating | 4.5");
        assert!(easels.add_text(id, "   ").is_err());
        assert!(easels.add_text(id + 1, "x").is_err());
    }

    #[test]
    fn capture_keeps_the_link_and_only_accepts_web_pages() {
        let (mut easels, id) = board();
        let clip = easels
            .capture(id, "https://example.com/a?b=1", "Example", " hello ")
            .unwrap();
        let item = &easels.get(id).unwrap().items[0];
        assert_eq!((item.id, item.kind), (clip, ItemKind::Clip));
        assert_eq!(item.url.as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(item.text, "hello");
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "about:blank",
            "x",
        ] {
            assert!(easels.capture(id, bad, "t", "").is_err(), "{bad}");
        }
        assert!(easels.capture(99, "https://a.com", "t", "").is_err());
        assert_eq!(easels.get(id).unwrap().items.len(), 1);
    }

    #[test]
    fn capture_without_title_falls_back_to_the_address() {
        let (mut easels, id) = board();
        easels.capture(id, "https://a.com/", " ", "").unwrap();
        assert_eq!(easels.get(id).unwrap().items[0].title, "https://a.com/");
    }

    #[test]
    fn items_are_laid_out_without_overlapping_and_ids_are_unique() {
        let (mut easels, id) = board();
        let a = easels.add_note(id, None).unwrap();
        let b = easels.add_note(id, None).unwrap();
        assert_ne!(a, b);
        let items = &easels.get(id).unwrap().items;
        assert_ne!((items[0].x, items[0].y), (items[1].x, items[1].y));
        easels.remove_item(id, a);
        let c = easels.add_note(id, None).unwrap();
        assert!(c != b);
    }

    #[test]
    fn edits_are_clamped_and_unknown_targets_are_ignored() {
        let (mut easels, id) = board();
        let n = easels.add_note(id, Some((f64::NAN, 5.0))).unwrap();
        assert_eq!(easels.get(id).unwrap().items[0].x, 0.0);
        assert!(easels.move_item(id, n, 1e12, -1e12));
        assert!(easels.resize_item(id, n, 1.0, 1e9));
        let i = &easels.get(id).unwrap().items[0];
        assert_eq!((i.x, i.y), (COORD_LIMIT, -COORD_LIMIT));
        assert_eq!((i.w, i.h), (MIN_SIZE.0, MAX_SIZE.1));
        assert!(easels.set_text(id, n, &"é".repeat(MAX_TEXT + 10)));
        assert_eq!(
            easels.get(id).unwrap().items[0].text.chars().count(),
            MAX_TEXT
        );
        assert!(!easels.set_color(id, n, "red;background:url(x)"));
        assert!(easels.set_color(id, n, COLORS[2]));
        assert!(!easels.move_item(id, 999, 0.0, 0.0));
        assert!(!easels.move_item(999, n, 0.0, 0.0));
    }

    #[test]
    fn live_capture_sizes_the_card_after_the_region() {
        let (mut easels, id) = board();
        let live = easels
            .capture_live(
                id,
                "https://a.com/p",
                "",
                "main > h1:nth-of-type(2)",
                (480.0, 240.0),
            )
            .unwrap();
        let item = &easels.get(id).unwrap().items[0];
        assert_eq!((item.id, item.kind), (live, ItemKind::Live));
        assert_eq!(item.selector, "main > h1:nth-of-type(2)");
        assert_eq!((item.w, item.h), (480.0, 240.0 + BAR_HEIGHT));
        assert_eq!(item.title, "https://a.com/p");
        easels
            .capture_live(id, "https://a.com", "t", "x", (f64::NAN, 0.0))
            .unwrap();
        let odd = &easels.get(id).unwrap().items[1];
        assert!(odd.w >= 240.0 && odd.h >= MIN_SIZE.1);
    }

    #[test]
    fn easels_can_be_filed_in_a_folder() {
        let (mut easels, id) = board();
        assert!(easels.set_folder(id, Some(7)));
        assert!(!easels.set_folder(id, Some(7)));
        assert_eq!(easels.get(id).unwrap().folder, Some(7));
        assert!(easels.set_folder(id, None));
        assert!(!easels.set_folder(99, Some(1)));
    }

    #[test]
    fn live_capture_rejects_bad_input() {
        let (mut easels, id) = board();
        let ok = (100.0, 100.0);
        assert!(easels
            .capture_live(id, "javascript:1", "t", "h1", ok)
            .is_err());
        assert!(easels
            .capture_live(id, "https://a.com", "t", "  ", ok)
            .is_err());
        assert!(easels
            .capture_live(id, "https://a.com", "t", "a\nb", ok)
            .is_err());
        assert!(easels
            .capture_live(9, "https://a.com", "t", "h1", ok)
            .is_err());
        let long = "a".repeat(MAX_SELECTOR + 50);
        easels
            .capture_live(id, "https://a.com", "t", &long, ok)
            .unwrap();
        assert_eq!(
            easels.get(id).unwrap().items[0].selector.len(),
            MAX_SELECTOR
        );
    }

    #[test]
    fn deleting_an_easel_removes_its_items() {
        let (mut easels, id) = board();
        easels.add_note(id, None);
        assert!(easels.delete(id));
        assert!(easels.get(id).is_none() && !easels.delete(id));
    }

    #[test]
    fn a_live_view_shows_the_page_at_the_width_it_was_picked_in() {
        let (mut easels, id) = board();
        let live = easels
            .capture_live(id, "https://a.com/p", "", "p", (600.0, 120.0))
            .unwrap();
        let viewport = |e: &Easels| e.get(id).unwrap().items[0].viewport;
        assert_eq!(viewport(&easels), 0.0);
        assert!(easels.set_viewport(id, live, 1200.0));
        assert_eq!(viewport(&easels), 1200.0);
        // A card 600 wide shows a page 1200 wide at half its size: same layout, same shape.
        assert_eq!(live_zoom(1200.0, 600.0), 0.5);
        assert_eq!(live_zoom(1200.0, 1200.0), 1.0);
        // Unknown width: as before. Extreme ratios stay within what an engine accepts.
        assert_eq!(live_zoom(0.0, 600.0), 1.0);
        assert_eq!(live_zoom(1200.0, f64::NAN), 1.0);
        assert_eq!(live_zoom(8000.0, 240.0), LIVE_ZOOM.0);
        assert_eq!(live_zoom(200.0, 1200.0), LIVE_ZOOM.1);
        // The page reports the width: nonsense is dropped.
        for bad in [f64::NAN, f64::INFINITY, -5.0, 10.0, 1e9] {
            assert!(easels.set_viewport(id, live, bad));
            assert_eq!(viewport(&easels), 0.0);
        }
        assert!(!easels.set_viewport(id, 999, 1200.0));
    }

    #[test]
    fn stored_data_is_sanitised_on_load() {
        let bad = |kind, url: Option<&str>| Item {
            id: 1,
            kind,
            x: f64::INFINITY,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            text: String::new(),
            title: String::new(),
            url: url.map(str::to_owned),
            selector: "h1".into(),
            color: "javascript:x".into(),
            viewport: f64::NAN,
        };
        let easels = Easels::from_stored(vec![Easel {
            id: 3,
            name: " A ".into(),
            folder: None,
            space: None,
            items: vec![
                bad(ItemKind::Clip, Some("javascript:alert(1)")),
                bad(ItemKind::Clip, Some("https://ok.com")),
                bad(ItemKind::Note, None),
                bad(ItemKind::Live, Some("https://ok.com")),
            ],
        }]);
        let e = easels.get(3).unwrap();
        assert_eq!(e.name, "A");
        assert_eq!(e.items.len(), 3);
        assert!(e.items.iter().all(|i| i.x == 0.0 && i.color == COLORS[0]));
        let id = Easels::from_stored(vec![]).create("n");
        assert_eq!(id, Some(1));
    }
}
