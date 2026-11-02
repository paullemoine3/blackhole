/// Release notes shown once on the start page after an update, newest first.
pub const RELEASES: &[(&str, &[&str])] = &[
    (
        "0.7.2",
        &[
            "Library (Arc-style): media, downloads, easels & notes, spaces and archived tabs; closed tabs are archived and can be reopened",
            "Downloads are saved to the Downloads folder and listed in the Library",
            "The sidebar no longer lists loose Easels (open them from the Library or the command palette); the Library button sits in the footer",
        ],
    ),
    (
        "0.7.1",
        &[
            "The sidebar scrolls with a themed scrollbar when it overflows; the spaces bar stays fixed at the bottom",
            "Bookmarks and Easels now belong to a space, like pinned tabs, the grid and tabs",
            "Imported bookmarks become real folders and tabs (drag & drop, close, delete) instead of a separate list",
            "Folders can be reordered by drag & drop",
        ],
    ),
    (
        "0.7.0",
        &[
            "An Easel is now a tab: click it in the sidebar to show it in the content area, like a web page, with its own controls",
            "Bookmarks appear directly in the Pinned section, ahead of folders and tabs",
        ],
    ),
    (
        "0.6.0",
        &[
            "Dragging a tab into the Pinned area is easier: bigger drop zone, with a hint when empty",
            "Swiping between spaces is more reliable, and the sidebar background fades to the new space colour",
        ],
    ),
    (
        "0.5.4",
        &[
            "Easels now appear in the sidebar like tabs: click to open, drag them into folders",
            "Right-click on the sidebar background to create a tab, folder, Easel or space; folders get their own menu (emoji, collapse, delete)",
        ],
    ),
    (
        "0.5.3",
        &[
            "Easel live capture: pick an area of a page (Cmd/Ctrl + Shift + L) and it stays live and interactive in an Easel card",
        ],
    ),
    (
        "0.5.2",
        &[
            "Easel: free-form boards for notes and clips of web pages, in their own window (Cmd/Ctrl + Shift + E)",
            "Capture the page in front to an Easel with Cmd/Ctrl + Shift + S or from the command bar; each clip links back to its page",
        ],
    ),
    (
        "0.5.1",
        &[
            "Installed extensions can now be allowed in private windows (Settings → Extensions)",
            "The \"Private\" badge no longer overlaps the address bar",
            "Local network addresses (e.g. http://192.168.1.90:7878/) keep http and their port, whether pasted or typed",
            "Drag and drop of tabs, grid tabs and spaces shows a dotted card where the item will land",
            "Sidebar and top-bar icons are properly centered",
            "Tabs now have three states: quick-access grid, pinned (folders and the tabs inside them) or regular",
            "Pinned folders sit in their own section, with a separator above your regular tabs",
        ],
    ),
    (
        "0.5.0",
        &["Pinned tabs are immutable: searching from one opens a new tab instead of replacing it"],
    ),
    (
        "0.4.0",
        &[
            "Slimmer top bar",
            "Swipe with two fingers on the sidebar to switch space",
            "Drag and drop spaces to reorder them",
            "Pinned tabs sit at the top of the sidebar, separated from your other tabs",
            "A pinned tab no longer has to be in the favorites grid, and the other way round",
            "This \"What's new\" page, shown once after each update",
        ],
    ),
    (
        "0.3.0",
        &["Built-in updater, command bar and split view improvements"],
    ),
];

fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    Some((next()?, next()?, next()?))
}

/// Notes of every release newer than `last_seen` up to `current`, newest first.
/// An empty `last_seen` (settings written by a version that did not track it) yields only the
/// `current` release. A first-ever launch (`fresh`) shows nothing: nothing is "new" to a new user.
pub fn pending(
    last_seen: &str,
    current: &str,
    fresh: bool,
) -> Vec<(&'static str, &'static [&'static str])> {
    let Some(current_v) = parse(current) else {
        return Vec::new();
    };
    if fresh {
        return Vec::new();
    }
    let seen = parse(last_seen);
    RELEASES
        .iter()
        .filter(|(v, _)| match (parse(v), seen) {
            (Some(v), Some(seen)) => v > seen && v <= current_v,
            (Some(v), None) => v == current_v,
            _ => false,
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_releases_newer_than_the_last_seen_one() {
        let notes = pending("0.3.0", "0.4.0", false);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "0.4.0");
        assert_eq!(pending("0.2.0", "0.4.0", false).len(), 2);
    }

    #[test]
    fn shows_nothing_when_up_to_date_or_on_first_launch() {
        assert!(pending("0.4.0", "0.4.0", false).is_empty());
        assert!(pending("", "0.4.0", true).is_empty());
        assert!(pending("0.4.0", "garbage", false).is_empty());
    }

    #[test]
    fn existing_users_without_a_marker_see_the_current_release() {
        let notes = pending("", "0.4.0", false);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].0, "0.4.0");
    }

    #[test]
    fn releases_are_ordered_newest_first() {
        let versions: Vec<_> = RELEASES.iter().map(|(v, _)| parse(v).unwrap()).collect();
        assert!(versions.windows(2).all(|w| w[0] > w[1]));
    }
}
