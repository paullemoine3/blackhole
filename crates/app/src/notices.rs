//! Notices: the one way the shell says what just happened. The kinds (icon, tone, time) are
//! declared in `domain::notice`; the pages draw them with one component (`ui/notice.js`): the
//! sidebar's toast while a sidebar is on screen, the top bar's address field while it is hidden.

use serde_json::{json, Value};

use blackhole_domain::notice;
pub(crate) use blackhole_domain::notice::Kind as Notice;

use crate::{i18n, Running};

impl Running {
    /// Shows a notice of `kind`. `message` may come from another layer (`i18n::message`);
    /// `actions` is a list of `{label, run}` buttons (`run`: a `UiCommand`), or `Value::Null`.
    pub(crate) fn notify(&self, kind: Notice, message: &str, actions: Value) {
        let asking = actions.as_array().is_some_and(|a| !a.is_empty());
        let notice = json!({
            "text": i18n::render(message),
            "icon": kind.icon(),
            "tone": kind.tone().id(),
            "stay": kind.stay_ms(asking),
            "actions": if asking { actions } else { json!([]) },
        });
        // Every host gets it and keeps its time: the sidebar pages always draw it, the top bar
        // only while no sidebar is on screen, and takes over if the sidebar leaves meanwhile.
        self.to_notice_hosts(&format!("window.bh&&bh.notice&&bh.notice({notice})"));
    }

    /// A notice with one button (`label`) that runs `run`, a `UiCommand`.
    pub(crate) fn notify_action(&self, kind: Notice, message: &str, label: &str, run: Value) {
        self.notify(kind, message, json!([{"label": label, "run": run}]));
    }

    /// A button of the notice was pressed in one host: the others drop it too.
    pub(crate) fn notice_done(&self) {
        self.to_notice_hosts("window.bh&&bh.noticeDone&&bh.noticeDone()");
    }

    fn to_notice_hosts(&self, js: &str) {
        let _ = self.bar.evaluate_script(js);
        self.ui(js.to_owned());
    }

    /// What the top bar needs to place notices: whether a sidebar is on screen (it shows them
    /// then), and how many things wait in a hidden one (the dot on the sidebar button).
    pub(crate) fn bar_notice_state(&self) -> (bool, usize) {
        let side = !notice::in_top_bar(self.sidebar.is_open(), self.sidebar.flyout_shown());
        let waiting = self.dl_live.values().filter(|d| d.finished.is_none()).count()
            + self.signals.all().len();
        (side, waiting)
    }

    /// Tells the top bar when that changed (called on every layout and state refresh).
    pub(crate) fn sync_bar_notice(&self) {
        let now = self.bar_notice_state();
        if self.bar_notice.replace(now) != now {
            let (side, waiting) = now;
            let _ = self
                .bar
                .evaluate_script(&format!("window.bh&&bh.side&&bh.side({side},{waiting})"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_names_an_icon_of_the_table() {
        let table = include_str!("../ui/icons.js");
        for kind in Notice::ALL {
            let icon = kind.icon();
            assert!(
                table.contains(&format!("\n    {icon}: ")) || table.contains(&format!("\n    '{icon}': ")),
                "{icon} is not in ui/icons.js"
            );
        }
    }
}
