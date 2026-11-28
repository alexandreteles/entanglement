use super::{Family, Framework};
use std::collections::BTreeSet;

/// Collect SDK compilation symbols for the target framework and its explicit platform version.
pub fn symbols(tfm: &str, platform_version: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let Some(target) = Framework::parse(tfm) else {
        return result;
    };
    let (framework, platform) = tfm.split_once('-').unwrap_or((tfm, ""));
    let name = framework.to_ascii_uppercase().replace('.', "_");
    result.extend([name.clone(), format!("{name}_OR_GREATER")]);
    if !target.platform.is_empty() {
        let platform_name = target.platform.to_ascii_uppercase();
        result.insert(platform_name.clone());
        let explicit = &platform[target.platform.len()..];
        let version = if platform_version.is_empty() {
            explicit
        } else {
            platform_version
        };
        if !version.is_empty() {
            let versioned = format!("{platform_name}{}", version.replace('.', "_"));
            result.extend([versioned.clone(), format!("{versioned}_OR_GREATER")]);
        }
    }
    let (major, minor, _) = target.version;
    if target.family == Family::Framework {
        result.insert("NETFRAMEWORK".into());
        let version = framework.trim_start_matches("net");
        for supported in [
            "20", "30", "35", "40", "45", "451", "452", "46", "461", "462", "47", "471", "472",
            "48", "481",
        ] {
            if supported <= version {
                result.insert(format!("NET{supported}_OR_GREATER"));
            }
        }
        return result;
    }
    let (prefix, versions): (_, &[(u16, u16)]) = match target.family {
        Family::Standard => (
            "NETSTANDARD",
            &[
                (1, 0),
                (1, 1),
                (1, 2),
                (1, 3),
                (1, 4),
                (1, 5),
                (1, 6),
                (2, 0),
                (2, 1),
            ],
        ),
        _ => (
            "NETCOREAPP",
            &[(1, 0), (1, 1), (2, 0), (2, 1), (2, 2), (3, 0), (3, 1)],
        ),
    };
    result.insert(prefix.into());
    result.extend(
        versions
            .iter()
            .filter(|v| **v <= (major, minor))
            .map(|(a, b)| format!("{prefix}{a}_{b}_OR_GREATER")),
    );
    if target.family == Family::Net {
        result.insert("NET".into());
        result.extend((5..=major.min(100)).map(|v| format!("NET{v}_0_OR_GREATER")));
    }
    result
}
