//! Pilot split: one pane of the split view drives the other. A link clicked in the pilot pane
//! opens in the other pane, so a list (search results, a feed, a documentation index) stays in
//! place while its entries are read beside it. Pure rules; the state is session only.

use crate::TabId;
use url::Url;

/// The pane that receives the links of `pilot`: the other tab on screen, while `pilot` is one of
/// the two panes of the split (`active` on the left, `split` on the right).
pub fn target(pilot: Option<TabId>, active: Option<TabId>, split: Option<TabId>) -> Option<TabId> {
    let (pilot, active, split) = (pilot?, active?, split?);
    if pilot == active {
        Some(split)
    } else if pilot == split {
        Some(active)
    } else {
        None
    }
}

/// Whether a link clicked on `page` goes to the other pane: a web address that is another
/// document (an anchor of the page itself scrolls in place).
pub fn leaves_page(page: &Url, link: &Url) -> bool {
    if !matches!(link.scheme(), "http" | "https") {
        return false;
    }
    let bare = |u: &Url| {
        let mut u = u.clone();
        u.set_fragment(None);
        u
    };
    bare(page) != bare(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    #[test]
    fn the_pilot_drives_the_other_pane_whichever_side_it_is_on() {
        let (a, b, c) = (TabId(1), TabId(2), TabId(3));
        assert_eq!(target(Some(a), Some(a), Some(b)), Some(b));
        // The panes were swapped (the right one was activated): the pilot still drives the other.
        assert_eq!(target(Some(a), Some(b), Some(a)), Some(b));
        // No split, no pilot, or a pilot that is no longer on screen: nothing.
        assert_eq!(target(Some(a), Some(a), None), None);
        assert_eq!(target(None, Some(a), Some(b)), None);
        assert_eq!(target(Some(c), Some(a), Some(b)), None);
        assert_eq!(target(Some(a), None, Some(b)), None);
    }

    #[test]
    fn only_links_to_another_web_page_leave() {
        let page = u("https://news.test/list?page=2#top");
        assert!(leaves_page(&page, &u("https://news.test/item/1")));
        assert!(leaves_page(&page, &u("https://other.test/")));
        assert!(leaves_page(&page, &u("https://news.test/list?page=3")));
        assert!(!leaves_page(&page, &u("https://news.test/list?page=2#comments")));
        assert!(!leaves_page(&page, &u("https://news.test/list?page=2")));
        assert!(!leaves_page(&page, &u("mailto:a@b.test")));
        assert!(!leaves_page(&page, &u("javascript:void(0)")));
    }
}
