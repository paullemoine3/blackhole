//! Which tabs keep a live WebView. Each one is a renderer: a phone keeps few of them.

use blackhole_domain::overflow_to_release;

/// Live views, least recently used released first.
#[derive(Debug, Default)]
pub struct Live {
    views: Vec<(u64, u64)>,
    clock: u64,
}

impl Live {
    pub fn contains(&self, tab: u64) -> bool {
        self.views.iter().any(|(t, _)| *t == tab)
    }

    /// `tab` is shown (and alive): returns the tabs whose views must go to stay under `max`.
    pub fn touch(&mut self, tab: u64, max: usize) -> Vec<u64> {
        self.clock += 1;
        match self.views.iter_mut().find(|(t, _)| *t == tab) {
            Some(entry) => entry.1 = self.clock,
            None => self.views.push((tab, self.clock)),
        }
        let gone = overflow_to_release(&self.views, &[tab], max.max(1));
        self.views.retain(|(t, _)| !gone.contains(t));
        gone
    }

    /// Forgets a view the shell destroyed; returns whether it was alive.
    pub fn remove(&mut self, tab: u64) -> bool {
        let before = self.views.len();
        self.views.retain(|(t, _)| *t != tab);
        before != self.views.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_shown_view_goes_first() {
        let mut live = Live::default();
        assert!(live.touch(1, 2).is_empty());
        assert!(live.touch(2, 2).is_empty());
        live.touch(1, 2);
        assert_eq!(live.touch(3, 2), vec![2]);
        assert!(live.contains(1) && live.contains(3) && !live.contains(2));
        assert!(live.remove(3));
        assert!(!live.remove(3));
    }

    #[test]
    fn the_shown_tab_always_stays() {
        let mut live = Live::default();
        live.touch(1, 1);
        assert_eq!(live.touch(2, 0), vec![1]);
        assert!(live.contains(2));
    }
}
