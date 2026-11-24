//! The Safari version BlackHole announces on macOS: the one of the WebKit engine it runs on.
//! Sites such as Google's only support the last two Safari versions, so a frozen number ends up
//! refused whatever the engine can do, and a user agent without a browser name is refused at once.

/// Never announced lower: the version BlackHole announced before it followed the system.
const FLOOR: (u32, u32) = (18, 5);

/// First Safari version of each WebKit major build.
const BUILDS: [(u32, (u32, u32)); 8] = [
    (625, (27, 0)),
    (624, (26, 4)),
    (623, (26, 2)),
    (622, (26, 0)),
    (621, (18, 4)),
    (620, (18, 2)),
    (619, (18, 0)),
    (618, (17, 4)),
];

/// The Safari version of the engine: macOS ships both under the same number since macOS 26, and
/// the build of `WebKit.framework` (`21624.5.1.11.3`: 21 for the system, 624 for WebKit) tells
/// of an engine updated on an older system.
#[must_use]
pub fn version(os: (u32, u32), webkit_build: &str) -> (u32, u32) {
    let by_system = match os.0 {
        // The system calls itself 16 to programs built with an older SDK.
        16 => (26, os.1),
        26.. => os,
        12..=15 => (os.0 + 3, os.1),
        _ => (0, 0),
    };
    let major = webkit_build
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .map_or(0, |m| m % 1000);
    let by_build = BUILDS
        .iter()
        .find(|(build, _)| major >= *build)
        .map_or((0, 0), |(_, version)| *version);
    by_system.max(by_build).max(FLOOR)
}

/// What follows WebKit's own user agent in Safari.
#[must_use]
pub fn application_name((major, minor): (u32, u32)) -> String {
    format!("Version/{major}.{minor} Safari/605.1.15")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_of_the_system_gives_the_version() {
        // Safari itself may be newer (27.2 here): the engine is what pages run on.
        assert_eq!(version((26, 7), "21624.5.1.11.3"), (26, 7));
        assert_eq!(version((27, 2), "22625.2.8"), (27, 2));
        assert_eq!(version((16, 1), "21622.2.11"), (26, 1));
        assert_eq!(version((15, 6), "20621.3.11"), (18, 6));
    }

    #[test]
    fn an_engine_updated_on_an_older_system_counts() {
        assert_eq!(version((15, 7), "20622.1.1"), (26, 0));
    }

    #[test]
    fn never_lower_than_before_nor_empty() {
        assert_eq!(version((12, 7), "17613.3.9"), FLOOR);
        assert_eq!(version((0, 0), ""), FLOOR);
        assert_eq!(application_name((26, 7)), "Version/26.7 Safari/605.1.15");
    }
}
