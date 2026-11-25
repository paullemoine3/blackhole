//! Erasing browsing data: what is erased and how far back.

/// A kind of browsing data the user can erase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataKind {
    /// History, with what Recall and the command bar learned from it.
    History,
    /// Cookies and what sites store (local storage, databases, service workers).
    Cookies,
    /// Images and files of the pages.
    Cache,
    /// The list of downloads (the files stay).
    Downloads,
    /// Closed tabs kept in the Library.
    Archive,
    /// What sites were allowed or refused.
    Permissions,
}

impl DataKind {
    pub const ALL: [Self; 6] =
        [Self::History, Self::Cookies, Self::Cache, Self::Downloads, Self::Archive, Self::Permissions];

    pub fn id(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Cookies => "cookies",
            Self::Cache => "cache",
            Self::Downloads => "downloads",
            Self::Archive => "archive",
            Self::Permissions => "permissions",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// Erased by the engine, in the profile of each space.
    pub fn in_engine(self) -> bool {
        matches!(self, Self::Cookies | Self::Cache)
    }
}

/// How far back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Hour,
    Day,
    Week,
    Month,
    All,
}

impl Period {
    pub const ALL: [Self; 5] = [Self::Hour, Self::Day, Self::Week, Self::Month, Self::All];

    pub fn id(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::All => "all",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The date (seconds since the epoch) from which data is erased; `None`: all of it.
    pub fn since(self, now: u64) -> Option<u64> {
        let secs = match self {
            Self::Hour => 3_600,
            Self::Day => 86_400,
            Self::Week => 7 * 86_400,
            Self::Month => 28 * 86_400,
            Self::All => return None,
        };
        Some(now.saturating_sub(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_go_back_from_now_and_all_has_no_start() {
        assert_eq!(Period::Hour.since(10_000), Some(6_400));
        assert_eq!(Period::Month.since(5), Some(0));
        assert_eq!(Period::All.since(10_000), None);
        for p in Period::ALL {
            assert_eq!(Period::from_id(p.id()), Some(p));
        }
        for k in DataKind::ALL {
            assert_eq!(DataKind::from_id(k.id()), Some(k));
        }
        assert_eq!(DataKind::from_id("passwords"), None, "never erased from here");
    }
}
