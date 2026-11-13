//! What can be done with a thing BlackHole shows: one rule for every menu (page, link, tab,
//! selection, highlight, download) and for the command bar, so that they cannot drift apart. A
//! menu asks `verbs(target, context)` and only draws the entries it gets; the shell asks
//! `allows` again before it acts, whatever the entry or shortcut that led there.

/// The thing a menu or an action is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The page shown in a tab.
    Page,
    /// A tab of the sidebar (its address), shown or not.
    Tab,
    /// A link of a page.
    Link,
    /// Text selected in a page.
    Selection,
    /// A highlight kept in the Library.
    Highlight,
    /// A file of the Library's downloads.
    Download,
    /// A result of a search (Recall, a watched area, the archive): an address and some text.
    Found,
}

/// Where the thing is: everything a rule needs, read by the shell from the engine and the
/// workspace (never from the page).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// The page is the one in front: not the other pane of a split, not a Peek, not the start
    /// page.
    pub front: bool,
    /// A web address (http or https).
    pub web: bool,
    /// It comes from a black hole tab: nothing of it may be kept or leave this window.
    pub burner: bool,
    /// BlackHole's error page is shown instead of the site.
    pub error: bool,
    /// At least one device is paired ("Continue on", the Wormhole).
    pub paired: bool,
    /// A download that finished well (its file can be opened or sent).
    pub ready: bool,
}

/// What can be done with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Open it (a tab, the file).
    Open,
    /// Keep it in an Easel (a clip, a note).
    Keep,
    /// Capture a live area of it into an Easel.
    LiveArea,
    /// Watch an area of it for changes.
    Watch,
    /// Mark it as a highlight.
    Highlight,
    /// Send it to a paired device ("Continue on", the Wormhole).
    Send,
}

/// The verbs offered for `target` in `context`, in the order of the menus.
#[must_use]
pub fn verbs(target: Target, ctx: Context) -> Vec<Verb> {
    use Verb::*;
    // A black hole tab keeps nothing and sends nothing: its pages only ever open.
    let keeps = !ctx.burner;
    let sends = !ctx.burner && ctx.paired;
    let mut out = Vec::new();
    match target {
        Target::Page => {
            if !ctx.web || ctx.error {
                return out;
            }
            if ctx.front && keeps {
                out.extend([Keep, LiveArea, Watch]);
            }
            if sends {
                out.push(Send);
            }
        }
        Target::Tab => {
            if ctx.web && sends {
                out.push(Send);
            }
        }
        Target::Link => {
            if !ctx.web {
                return out;
            }
            out.push(Open);
            if keeps {
                out.push(Keep);
            }
            if sends {
                out.push(Send);
            }
        }
        Target::Selection => {
            if ctx.web && keeps && !ctx.error {
                out.push(Highlight);
            }
            if sends {
                out.push(Send);
            }
        }
        Target::Highlight | Target::Found => {
            out.extend([Open, Keep]);
            if sends {
                out.push(Send);
            }
        }
        Target::Download => {
            if ctx.ready {
                out.push(Open);
                if sends {
                    out.push(Send);
                }
            }
        }
    }
    out
}

/// Whether `verb` is allowed for `target` in `context`.
#[must_use]
pub fn allows(target: Target, ctx: Context, verb: Verb) -> bool {
    verbs(target, ctx).contains(&verb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Context {
        Context {
            front: true,
            web: true,
            ..Context::default()
        }
    }

    #[test]
    fn the_page_in_front_can_be_kept_captured_and_watched() {
        assert_eq!(
            verbs(Target::Page, page()),
            [Verb::Keep, Verb::LiveArea, Verb::Watch]
        );
        let paired = Context { paired: true, ..page() };
        assert!(allows(Target::Page, paired, Verb::Send));
    }

    #[test]
    fn a_black_hole_page_keeps_nothing_and_sends_nothing() {
        let hole = Context {
            burner: true,
            paired: true,
            ..page()
        };
        assert!(verbs(Target::Page, hole).is_empty());
        assert!(verbs(Target::Tab, hole).is_empty());
        assert_eq!(verbs(Target::Link, hole), [Verb::Open]);
        assert!(verbs(Target::Selection, hole).is_empty());
    }

    #[test]
    fn another_pane_an_error_page_or_a_local_page_get_no_tool() {
        let side = Context { front: false, ..page() };
        assert!(!allows(Target::Page, side, Verb::Keep));
        assert!(!allows(Target::Page, side, Verb::LiveArea));
        let error = Context { error: true, ..page() };
        assert!(verbs(Target::Page, error).is_empty());
        let local = Context { web: false, ..page() };
        assert!(verbs(Target::Page, local).is_empty());
        assert!(verbs(Target::Link, local).is_empty());
    }

    #[test]
    fn sending_needs_a_paired_device() {
        for target in [Target::Tab, Target::Link, Target::Selection, Target::Highlight, Target::Found] {
            assert!(!allows(target, page(), Verb::Send), "{target:?}");
            assert!(allows(target, Context { paired: true, ..page() }, Verb::Send), "{target:?}");
        }
    }

    #[test]
    fn a_download_is_opened_or_sent_once_it_is_there() {
        let paired = Context { paired: true, ..Context::default() };
        assert!(verbs(Target::Download, paired).is_empty());
        let ready = Context { ready: true, ..paired };
        assert_eq!(verbs(Target::Download, ready), [Verb::Open, Verb::Send]);
    }

    #[test]
    fn what_was_found_or_highlighted_can_go_to_the_easel() {
        assert!(allows(Target::Found, Context::default(), Verb::Keep));
        assert!(allows(Target::Highlight, Context::default(), Verb::Keep));
        assert!(allows(Target::Link, page(), Verb::Keep));
    }
}
