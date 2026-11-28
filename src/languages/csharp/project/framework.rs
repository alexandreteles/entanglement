use super::View;
mod defines;
pub(super) use defines::symbols;

#[derive(PartialEq, Eq)]
enum Family {
    Framework,
    Standard,
    Core,
    Net,
}
struct Framework<'a> {
    family: Family,
    version: (u16, u16, u16),
    platform: &'a str,
    platform_version: Vec<u16>,
}

impl<'a> Framework<'a> {
    fn parse(tfm: &'a str) -> Option<Self> {
        let (tfm, suffix) = tfm.split_once('-').unwrap_or((tfm, ""));
        let platform = suffix.trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '.');
        let platform_version = suffix[platform.len()..]
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let (family, version) = if let Some(v) = tfm.strip_prefix("netstandard") {
            (Family::Standard, v)
        } else if let Some(v) = tfm.strip_prefix("netcoreapp") {
            (Family::Core, v)
        } else {
            let v = tfm.strip_prefix("net")?;
            (
                if v.contains('.') {
                    Family::Net
                } else {
                    Family::Framework
                },
                v,
            )
        };
        let numbers = if family == Family::Framework {
            version
                .chars()
                .map(|c| c.to_digit(10).map(|i| i as u16))
                .collect::<Option<Vec<_>>>()?
        } else {
            version
                .split('.')
                .map(|s| s.parse::<u16>().ok())
                .collect::<Option<Vec<_>>>()?
        };
        Some(Self {
            family,
            version: (
                *numbers.first()?,
                *numbers.get(1).unwrap_or(&0),
                *numbers.get(2).unwrap_or(&0),
            ),
            platform,
            platform_version,
        })
    }
    fn rank(&self, library: &Self) -> Option<(u8, bool, (u16, u16, u16))> {
        if !library.platform.is_empty()
            && (library.platform != self.platform
                || !self.platform_version.is_empty()
                    && library.platform_version > self.platform_version)
        {
            return None;
        }
        let family = if self.family == library.family && library.version <= self.version {
            3
        } else if self.family == Family::Net
            && library.family == Family::Core
            && library.version <= (3, 1, 0)
        {
            2
        } else if library.family == Family::Standard && library.version <= self.standard()? {
            1
        } else {
            return None;
        };
        Some((family, !library.platform.is_empty(), library.version))
    }
    fn standard(&self) -> Option<(u16, u16, u16)> {
        let version = match self.family {
            Family::Net => (2, 1, 0),
            Family::Core if self.version.0 >= 3 => (2, 1, 0),
            Family::Core if self.version.0 >= 2 => (2, 0, 0),
            Family::Core => (1, 6, 0),
            Family::Framework if self.version >= (4, 6, 1) => (2, 0, 0),
            Family::Framework if self.version >= (4, 6, 0) => (1, 3, 0),
            Family::Framework if self.version >= (4, 5, 1) => (1, 2, 0),
            Family::Framework if self.version >= (4, 5, 0) => (1, 1, 0),
            _ => return None,
        };
        Some(version)
    }
}

/// Keep unknown custom frameworks as possible targets when no known compatible target exists.
pub(super) fn nearest(request: &str, options: &[(usize, &View)]) -> Vec<usize> {
    let exact = options
        .iter()
        .filter(|(_, v)| v.framework == request)
        .map(|(i, _)| *i)
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return exact;
    }
    let Some(request) = Framework::parse(request) else {
        return options.iter().map(|(i, _)| *i).collect();
    };
    let ranked = options
        .iter()
        .filter_map(|(i, v)| {
            request
                .rank(&Framework::parse(&v.framework)?)
                .map(|rank| (rank, *i))
        })
        .collect::<Vec<_>>();
    if let Some(best) = ranked.iter().map(|(rank, _)| *rank).max() {
        return ranked
            .into_iter()
            .filter(|(r, _)| *r == best)
            .map(|(_, i)| i)
            .collect();
    }
    options
        .iter()
        .filter(|(_, v)| Framework::parse(&v.framework).is_none())
        .map(|(i, _)| *i)
        .collect()
}
