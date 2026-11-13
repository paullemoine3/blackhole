/// Release notes shown once on the start page after an update, newest first.
pub const RELEASES: &[(&str, &[&str])] = &[
    (
        "0.8.7",
        &[
            "Quick access and pinned tabs have an address of their own, like Arc: the page they had when you put them there. They can wander from it and come back to it when they wake up (after sleeping, or at the next launch); their menu offers Back to its own page, or Keep this page as its address",
            "A closed folder still shows the pages open under it, like Arc, so opening a tab from the command bar no longer unfolds its folders; a folder with no page open is dimmed",
            "Right click anywhere on a page opens BlackHole's own menu: back, forward, reload, add the page to the Easel, capture a live area, watch an area for changes, copy the clean link, find, print, inspect; an image adds open in a new tab and copy its address. Shift+right click, text fields, videos and pages with a menu of their own keep theirs",
            "A link that downloads a file no longer replaces its page with Site unreachable on Windows",
            "Error pages: Back to safety (and the Back button) now returns to the page you were on instead of bouncing on the address that failed, and closes a tab that has nothing to return to; Try again and Sign in really ask the server again, without piling entries in the history",
            "A refused certificate on a site opened from another page no longer makes the tab flicker between two error pages on Windows",
            "A hidden sidebar slides out for a few seconds when a download starts or ends, so its card is seen, and the page stays usable meanwhile",
            "The card of a finished download has a close button: it leaves the sidebar at once instead of waiting, with the same motion it came in with; the file and its line in the Library stay",
            "New launch animation, on the computer and on Android: the stars fall to a point, a flash, the black hole is born, the name comes letter by letter, then the hole lands on the start page's logo",
            "The start page's logo is quiet at rest and reacts: under the pointer (a touch on Android) the disc swells, a second ring races and the stars around it are swallowed, then come back gently",
            "The sidebar no longer flickers when the count of blocked requests moves: its rows are only redrawn when a tab, a folder or a space changes",
            "Motion, on the desktop and on Android: menu rows rise one after the other and a menu fades out when it closes; a toast rises, shows the time it stays and leaves downwards; the count of blocked requests rolls like a counter; a finished download draws a check; a folder's tabs unfold from under it and fold back. On the desktop the highlight of the active tab glides from the tab left to the tab shown",
            "A tab opened from a page (new-window link, Ctrl/Cmd+click, middle click) now tells the site which site it comes from, as the link would have: an image in a Reddit comment opens full screen instead of showing its post again",
            "Android is smoother: the interface no longer redraws itself at every page event, so nothing blinks while you type in the address bar, close a tab or load a page; suggestions change in place, and the keyboard of the address bar no longer moves the bar or resizes the page",
            "Android speaks French: the interface follows the system's language, or the one picked in Settings > Appearance > Language, and switches at once",
            "Android: a long press on a tab, a space or a folder opens its menu (quick access, rename, move to a folder or another space, close the other tabs, delete with Undo); folders show in the sidebar, fold, and a card offers to tidy up loose tabs by site",
            "Android: sign-in windows (Google, Microsoft, Apple, GitHub…) now return to the page that opened them, and a server that asks for a name and password gets them from a question in BlackHole, never stored",
            "Android: an empty 401, 403 or server error answer shows an error page instead of a blank one, and a page whose engine process stops is reloaded once instead of taking the app down",
        ],
    ),
    (
        "0.8.6",
        &[
            "Recall: find a page you read from a few words of its text (command bar, Ctrl/Cmd+Shift+F). Off until you turn it on in Settings > Privacy; the text stays on this computer, encrypted; never in private windows, black hole tabs, pages with a password field or the sites you list; clearing the history clears it too",
            "Black hole tab: a tab with a session of its own (no cookie of the space, nothing in the history or the archive), destroyed when it closes. From the command bar (New black hole tab) or a link's menu; four at most, since each keeps its page alive",
            "Pilot split: in a split view, the arrow on a pane makes its links open in the other pane, so a list of results or a table of contents stays in place while you read",
            "Clean links: more tracking parameters are dropped from addresses, the redirect pages of search engines and social networks are skipped, a link's menu copies the clean address, and Settings > Privacy has a switch for it",
            "Highlights: select text on a page, right-click, Highlight. The passage stays marked each time you come back, can carry a note, and is listed in Library > Highlights, from where it can be clipped to an Easel",
            "Wormhole: send selected text, the clipboard or a file (64 MB at most) to a paired device over the local network, encrypted, without a server. A file waits for Accept on the other device; a received text is only copied when you ask",
            "The command bar opens in one movement: the panel glides into place and its rows rise after it, with a single glow behind it for a new tab; the animations setting and the system's reduced motion now stop all of it",
        ],
    ),
    (
        "0.8.5",
        &[
            "Lighter on heavy web apps (YouTube, Google Docs and Sheets, Meet): trackers and ads are now blocked by the engine itself (WebKit content rules on macOS, request filters on Windows) instead of a script that wrapped the page's own functions in every frame",
            "Tabs that play nothing are no longer asked about their media every second and a half: a page tells the browser when something starts, stops or opens the camera or microphone",
            "A page whose engine process stops (out of memory on a heavy document, a crash) is reloaded once instead of staying blank; a background tab reloads when you come back to it",
            "A call is never moved to Picture in Picture when you switch tab or share your screen from another window",
            "macOS: the rounded corners of the page are no longer re-applied to every tab several times a second",
            "Picture in Picture on macOS: the floating window's back-to-tab button now shows the video's tab; its close button pauses the video and leaves you where you are",
            "The sidebar player's close button also closes the floating video, and the card stays closed until the video plays again",
            "Google can be picked as the search engine in Settings, on every platform; DuckDuckGo stays the default",
            "A click on a download card in the sidebar opens Library > Downloads, and a finished download stays there 8 seconds",
            "A tab opened only to download a file closes once the file is saved, instead of showing an error page",
            "The Library and the sidebar's player, download, meeting and tidy-up cards are redesigned on the BlackHole design system, with the same animations as Settings",
            "The sidebar's meeting card leaves 5 minutes after the meeting starts instead of staying until its end, and its icon turns into a cross to hide it at once",
            "Command bar: nothing is selected when it opens, so Enter on the untouched address reloads the page instead of jumping to the first tab of the list; the first row is selected as soon as you type",
            "Picture in Picture on macOS: back to tab also works for a call (Meet) and other muted videos, which WebKit pauses on their way back into a hidden tab",
        ],
    ),
    (
        "0.8.4",
        &[
            "BlackHole for Android updates itself: once a day (or with Check now in Settings > Updates) it looks for a new release on GitHub, then downloads the APK signed like the installed app, checks it against the release checksums and hands it to the system installer",
            "Android address bar: tapping it puts the whole address in the field, selected and ready to change; the bar shows the site's icon, its host and the path in grey; suggestions follow every key, and the go suggestion shows the whole address typed",
            "Site icons on Android everywhere a site appears: address bar, quick access, tabs, tab grid, history and suggestions (never kept for private tabs, forgotten with the history)",
            "Android browser basics: find in page, desktop site per tab, add to home screen, print or save as PDF, Stop while a page loads, pull to refresh and text size (Settings), and BlackHole as the default browser",
            "Android: long press an image to open, download or copy it, and a link to download it; full screen videos, file uploads, and the camera, microphone or location for a site only after you allow it",
            "Android: text shared from another app, a web search or \"BlackHole\" in a text selection menu opens its address or searches it",
            "Android fixes: tapping the address bar no longer closes the sheet at once (the keyboard typed into a hidden field), the page no longer stays black when the system recreates the app, a long press on a linked image offers the link instead of the image, and pages without a mobile layout are fitted to the screen",
        ],
    ),
    (
        "0.8.3",
        &[
            "Updates are offered again: BlackHole now finds a new version through the GitHub release page instead of its API, whose hourly quota is shared by everyone behind the same address (an office network used it up, so no update was ever proposed); a failed check is retried after 30 minutes",
            "Settings > General > Updates: see your version, check for a new one now, and install it from there",
            "Installing an update launches a small ship from the button, in the sidebar and in Settings (not with animations off or reduced motion)",
        ],
    ),
    (
        "0.8.2",
        &[
            "BlackHole speaks English and French: pick the language in Settings > General (or follow the system), and every open page switches at once, no restart",
            "Settings redesigned, with one icon size scale across the sidebar, menus and windows, built on the BlackHole design system",
            "\"Sign in with Google\" popups now return to the page that opened them, without the TrustedHTML error",
        ],
    ),
    (
        "0.8.1",
        &[
            "Fixed a crash on macOS when the Settings window lost the focus (switching app, clicking the browser window)",
            "Fixed a crash on macOS when a new tab failed to load its first page",
            "An address on a port browsers never use (9, 25, 6000…) shows \"Site injoignable\" instead of a blank tab",
            "Live areas of an Easel in its own window stay visible while a menu or the command bar is open in the browser window, and come back as soon as one closes when the Easel is a tab",
            "A live area appears at once in its card even when the Easel window is behind the browser",
            "Calendar: events you removed from Google Calendar (an old invitation declined or a cancelled event) are no longer offered as your next meeting",
            "Joining a calendar meeting offers to take its notes in an Easel beside the call; the Meet tab already in that room is reused",
        ],
    ),
    (
        "0.8.0",
        &[
            "BlackHole for Android: a lighter version for phones with tabs in a grid (swipe to close, Undo), the address bar at the bottom within reach of your thumb (swipe it to change tab), the sidebar as a drawer, the command bar as a sheet, quick access, history, private tabs, spaces with their color, Shields with the privacy report, downloads and the animated error pages",
            "The phone keeps the browser's rules: the same Shields lists, https by default, no backup of your browsing data, and private tabs whose cookies vanish with the last one",
            "The Android app is published with each release (BlackHole-android.apk), next to the Windows and macOS builds",
            "Continue on another device works with the phone too: pair BlackHole for Android with your computers (Settings > Devices) and send a tab either way",
            "Tab sync between paired devices: quick access, open tabs and spaces (name and color) stay the same on your computer and your phone. When a device comes back on the network BlackHole proposes to merge (Merge, Always, Later); with Always it merges on its own; a tab closed on one device closes on the other. Private tabs, folders and history stay on each device (Settings > Devices: Ask, Always or Off per device, and Sync now)",
            "Same motion on every device: on the start page the black hole breathes and its ring turns, switches in Settings spring over and glow when on, and the Shields count in the sidebar jumps when something new is blocked (all off with animations off or reduced motion)",
        ],
    ),
    (
        "0.7.19",
        &[
            "Continue on another device: pair two computers once with a code (Settings > Devices), then send a tab to the other one from its menu or the command bar; only its address travels, encrypted, on the local network (a YouTube or Twitch video resumes where it was)",
            "Right click on selected text: search it in a new tab with your search engine (Cmd/Ctrl+click or middle click: in the background), open it when it is a web address, or copy it",
            "Sites protected by a password of the browser (a NAS, a router, Transmission's web interface) ask for the user name and password instead of showing \"Site injoignable\"; a 401, 403 or 404 page from a server is shown as the server sends it",
            "Passwords (Settings > Passwords): saved logins encrypted on this device, offered on the login fields of the exact site (https) and filled only when you pick one; BlackHole offers to save a login after you sign in, with search, copy (cleared after 30 s), a generator, CSV import and an optional master password",
            "Error pages in the style of \"Site injoignable\" for each case: sign-in required (401) and access denied (403) or server error (5xx) when the server sends no page, unsafe connection (expired, self-signed or mismatched certificate, wrong computer clock) with its details and \"Continue anyway (not secure)\" under Advanced, for that site until BlackHole quits (never for HSTS sites, then \"Non sécurisé\" in the address bar), a site without https, a tracker blocked by the Shields (with \"Allow this site\"), too many redirects, offline and unknown address",
        ],
    ),
    (
        "0.7.18",
        &[
            "Locking a tab on a URL is gone: a quick access or folder tab simply comes back on the page you left it on",
            "Closing a quick access tab never takes it off the list (menu, Cmd/Ctrl+W, middle click): use Remove from quick access for that",
            "Middle click on a tab: a quick access or folder tab goes to sleep, another tab closes",
            "Quick access tiles no longer show a speaker over the icon, so a playing tab is easy to click",
            "Closing the floating video (Picture in Picture) no longer jumps to its tab; the player card stays in the sidebar",
            "The Settings and Easel windows have the same top bar as the browser window, in the space's color",
            "Middle click (or Cmd/Ctrl+click) on a link, a video title included, opens it in a new tab in the background",
            "Right click on a link: open it in a new tab, a new window or a private window, preview it or copy it",
            "After picking an area for a live capture, you choose the Easel it goes to (or a new one)",
            "Shields, as in Brave: ad servers are blocked besides trackers, and pages can hide their ad slots, skip YouTube ads and drop cookie banners, each one switched in Settings > Privacy",
            "Scroll bars of the menus, Settings, Library and Easel follow the theme",
            "New Google Meet (sidebar menu, command bar): reuses your Meet tab of quick access or of a folder, never one that is in a call",
            "Google Calendar sync (Settings > Calendar, with the calendar's secret iCal address, stored encrypted): the next meeting with a Join button in the sidebar, a reminder when it starts, Join from the command bar",
            "Developer tools for web pages: F12 or Ctrl+Shift+I (Cmd+Option+I on macOS), or Inspect in the tab menu: elements and styles, console, network",
            "Downloads (extension catalog, updates, calendars) trust the system's certificates, so they work behind an antivirus that inspects HTTPS",
            "Privacy report: the shield in the sidebar shows, for the site open, its grade (A to E), the trackers and ads it tried this session and the hosts blocked",
            "Tidy up: from 6 loose tabs, the sidebar offers to file the tabs of the busiest sites into one folder per site (with Undo)",
            "Meeting copilot: joining a calendar meeting opens its notes in an Easel, with the agenda and the attendees filled in",
            "Watch an area: after picking an area of a page, choose Watch this area for changes; it is read again every half hour and a change is told with the value before and after (Library > Watched Areas)",
            "Compare tabs: the products of the open tabs side by side (price, rating, reviews, brand, availability, shared specifications), lowest price and best rating highlighted",
            "Smoother sidebar: showing or hiding it now runs at about 120 frames per second, and the page slides along instead of being laid out again on every frame",
        ],
    ),
    (
        "0.7.17",
        &[
            "Tabs work like Arc: a new tab opens at the top of the list, and closing a tab brings back the one you saw just before",
            "New shortcuts: Ctrl+Tab goes back to the previous tab, Cmd/Ctrl+Alt+↓/↑ walks the sidebar and Cmd/Ctrl+Shift+T reopens the tab closed last",
            "Cmd/Ctrl+W on a pinned or folder tab puts it to sleep instead of removing it",
            "Library > History lists the pages you visited, with a search and a way to forget a page",
            "Locked tabs only reload when you have left the locked page, and the menu offers \"Back to the locked page\"",
            "Back on the first page of a new tab no longer lands on a blank page",
            "Windows: Ctrl+F (find in page) works again, as do F5 and Alt+←/→",
            "Find in page highlights every match with a counter, and you can keep typing while it searches; Google Docs keeps its own search",
            "Smoother sidebar and tab animations, and the memory / CPU meter no longer slows the browser",
            "The player card (YouTube, Twitch, Meet…) and the download cards in the sidebar no longer flicker or jump between players",
            "Deleting a space, a folder or an Easel, or clearing the history, asks for a second click; the tabs of a deleted space now go to the archive instead of being lost",
            "The button on a quick access or folder tab puts it to sleep instead of removing it; closing it for good is in its menu",
            "Menus use the same words and drawn icons everywhere, shortcuts are written the Windows way on Windows, and a folder can be renamed from its menu",
            "The tab menu can reload, duplicate, copy the link, mute, move the tab to another space and close the other tabs",
            "A space's color can be changed from its menu",
            "A deleted space or folder can be brought back with Undo for a few seconds",
            "The command bar lists find in page, print, zoom, new (private) window, reopen the last tab and History, with their shortcuts",
            "A stream that starts muted (Twitch) gets its player card, and its button turns the sound on",
            "Dragging and dropping inside a page also works right after closing the Library or an Easel",
        ],
    ),
    (
        "0.7.15",
        &[
            "A thin progress bar runs along the top of the page while it loads",
            "A new tab opened from a search shows a loading screen with the site's name instead of an empty background until its page is there",
            "Lower CPU use when the browser is idle",
            "Quick-access and folder tabs can be locked (right-click > Lock URL on this page): clicking them always brings you back to the locked page",
            "Cmd/Ctrl+R is now a hard refresh: the page is reloaded without its cache and service workers (you stay signed in)",
            "Google Docs loads again: pages that declare their own global `ipc` no longer break the browser's page bridge",
            "Picture-in-picture no longer starts when you work in another window on a second monitor while BlackHole stays visible",
            "Dragging and dropping inside a page works again (e.g. moving a Jira ticket between columns)",
        ],
    ),
    (
        "0.7.14",
        &[
            "New shortcuts, Cmd on macOS and Ctrl on Windows: zoom in / out / reset (+ − 0, remembered per site), find in page (F, then G / Shift+G for the next / previous match) and print (P)",
            "The address bar shows the zoom level of a zoomed page, with a button to reset it",
            "Neighbouring tabs are no longer opened in the background: a tab only loads when you open it",
        ],
    ),
    (
        "0.7.13",
        &[
            "The sidebar unfolds the folders and sections hiding the active tab, e.g. when a search opens a pinned tab",
            "Picture-in-picture on focus loss no longer triggers when you only move the window or click inside the browser",
            "The sidebar shows the number of active tabs and the memory and CPU used by BlackHole",
            "The hidden sidebar no longer flickers when pages load or tabs are created (macOS)",
            "Calls and live streams keep their tab alive more reliably when you browse elsewhere",
            "Screen sharing keeps its sharpness (resolution first) and the shadow under the page card fades in cleanly under the top bar",
            "Google Docs, Sheets and Slides load without the \"Loading issue\" page",
        ],
    ),
    (
        "0.7.12",
        &[
            "Blank tabs kept ready no longer leave an empty about:blank entry in a new tab's history",
            "The page card has rounder corners and a deeper shadow so it floats above the window",
            "Picture in Picture: the floating window's back-to-tab button now brings you back to the tab, and a playing video also floats when you leave the browser",
            "The sidebar player of a call (Google Meet…) can switch the microphone and the camera on and off",
            "Right-click a tab > Rename tab; tab titles are cut later, and quick-access tabs show the mute speaker over the icon on hover",
            "Tabs in a call or playing media are no longer freed when you browse elsewhere; Close tab on a quick-access or folder tab only frees the page and returns to the start page",
        ],
    ),
    (
        "0.7.10",
        &[
            "After a restart only the active tab is loaded: neighbouring tabs are no longer reopened until you start switching tabs",
        ],
    ),
    (
        "0.7.9",
        &[
            "Much lower memory use: only a few tabs stay alive at once (4 by default, adjustable in Settings), neighbouring tabs are preloaded and the least recent ones are released",
            "Hidden tabs run in a low-memory mode, and the Library, Easel and split-bar pages are only created when needed",
            "New setting: how many blank tabs (0, 1 or 2) are kept ready to open new tabs instantly",
            "The browser engine now shares renderer processes per site",
        ],
    ),
    (
        "0.7.8",
        &[
            "The back and forward buttons follow the real page history and are greyed out when there is nowhere to go",
            "New Privacy setting to switch the tracker blocker on or off (the https upgrade stays on)",
            "The Quick access tabs and the Pinned folders can be folded from their sidebar headers; the choice is remembered",
            "Faster start: session, history, icons and extensions load in parallel, and files are written in the background",
            "Faster navigation: the interface and the history are no longer rebuilt and rewritten on every page event",
        ],
    ),
    (
        "0.7.7",
        &[
            "Google Meet and other sites that check for Safari now accept BlackHole on macOS",
            "The macOS app asks for camera and microphone access for video calls",
        ],
    ),
    (
        "0.7.6",
        &[
            "Logins are scoped by space: sign in to a personal account in one space and a work account in another; login and refresh popups open in the space you are in",
            "Folders can contain subfolders (drag a folder onto another, or right-click → New subfolder); importing bookmarks keeps their folder hierarchy",
            "The sidebar mode (pinned open or shown on hover) is remembered between launches",
        ],
    ),
    (
        "0.7.5",
        &[
            "Spaces can be renamed from their right-click menu, which no longer offers to switch to the space you are already in",
            "The update button uses the same drawn icon on every system and no longer flickers on hover",
        ],
    ),
    (
        "0.7.4",
        &[
            "Smoother new tabs: blank tab views are prepared ahead, and the command bar animations are shorter",
            "Drag a tab onto another BlackHole window (private or not, same space included) to move it there, or onto nothing to open it in a new window",
            "Tabs playing sound show a speaker icon, and a mini player at the bottom of the sidebar controls them from another page",
            "Picture in Picture: a playing video floats when you leave its tab and returns with it; also available from the command bar",
        ],
    ),
    (
        "0.7.3",
        &[
            "Deleting a folder now also closes the tabs it contains instead of moving them to current tabs",
            "Drag & drop: a thin insertion line replaces the overlapping markers, and dragged folders no longer pile up",
            "Downloads show a progress card in the sidebar",
            "New tab opens the search bar over the start page; Home and app launch still show the start page alone",
        ],
    ),
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
