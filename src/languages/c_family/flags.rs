use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub(crate) struct Settings {
    pub directory: PathBuf,
    pub cpp: bool,
    pub quote: Vec<PathBuf>,
    pub include: Vec<PathBuf>,
    pub system: Vec<PathBuf>,
    pub forced: Vec<String>,
    pub macros: BTreeMap<String, Option<String>>,
}

impl Settings {
    pub fn standalone(path: &Path) -> Self {
        Self {
            cpp: !matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("c" | "h" | "inc")
            ),
            ..Self::default()
        }
    }

    pub fn from_args(file: &Path, directory: &Path, args: &[String]) -> Self {
        let mut flags = Self::standalone(file);
        flags.directory = directory.into();
        if args
            .first()
            .is_some_and(|arg| arg.ends_with("++") || arg.ends_with("++.exe"))
        {
            flags.cpp = true;
        }
        let mut args = args.iter().skip(1).peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "/TP" => flags.cpp = true,
                "/TC" => flags.cpp = false,
                _ => {
                    let Some(prefix) = [
                        "-iquote",
                        "-isystem",
                        "-include",
                        "/external:I",
                        "/FI",
                        "/Tp",
                        "/Tc",
                        "-I",
                        "/I",
                        "-D",
                        "/D",
                        "-U",
                        "/U",
                        "-x",
                        "-std=",
                        "/std:",
                    ]
                    .into_iter()
                    .find(|prefix| arg.starts_with(prefix)) else {
                        continue;
                    };
                    let suffix = &arg[prefix.len()..];
                    let value = if suffix.is_empty() {
                        args.next().map_or("", String::as_str)
                    } else {
                        suffix
                    };
                    match prefix {
                        "-x" => {
                            flags.cpp = if value == "none" {
                                Self::standalone(file).cpp
                            } else {
                                value.contains("++")
                            }
                        }
                        "/Tp" | "/Tc"
                            if crate::input::path_identity(&directory.join(value))
                                .ok()
                                .as_deref()
                                == Some(file) =>
                        {
                            flags.cpp = prefix == "/Tp"
                        }
                        "-I" | "/I" => flags.include.push(directory.join(value)),
                        "-iquote" => flags.quote.push(directory.join(value)),
                        "-isystem" | "/external:I" => flags.system.push(directory.join(value)),
                        "-include" | "/FI" => flags.forced.push(value.into()),
                        "-D" | "/D" => {
                            let (name, replacement) = value.split_once('=').unwrap_or((value, "1"));
                            flags.macros.insert(name.into(), Some(replacement.into()));
                        }
                        "-U" | "/U" => {
                            flags.macros.insert(value.into(), None);
                        }
                        "-std=" | "/std:" => {
                            if let Some(version) = cpp_standard(value) {
                                flags
                                    .macros
                                    .insert("__cplusplus".into(), Some(version.into()));
                            } else if let Some(version) = c_standard(value) {
                                flags
                                    .macros
                                    .insert("__STDC_VERSION__".into(), Some(version.into()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        flags
    }
}

fn cpp_standard(value: &str) -> Option<&'static str> {
    let standard = value.split_once("++")?.1;
    Some(match standard {
        "98" | "03" => "199711",
        "11" | "0x" => "201103",
        "14" | "1y" => "201402",
        "17" | "1z" => "201703",
        "20" | "2a" => "202002",
        "23" | "2b" => "202302",
        _ => return None,
    })
}

fn c_standard(value: &str) -> Option<&'static str> {
    let standard = value
        .strip_prefix("gnu")
        .or_else(|| value.strip_prefix('c'))?;
    Some(match standard {
        "99" | "9x" => "199901",
        "11" | "1x" => "201112",
        "17" | "18" => "201710",
        "23" | "2x" => "202311",
        _ => return None,
    })
}

/// Split a recorded command as arguments. Preserve Windows path separators.
pub(super) fn split_command(command: &str) -> Option<Vec<String>> {
    let windows = command.contains("cl.exe")
        || command.starts_with("cl ")
        || command.contains("clang-cl")
        || command.contains(":\\");
    if !windows {
        return shlex::split(command);
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' {
            let mut count = 1;
            while chars.next_if_eq(&'\\').is_some() {
                count += 1;
            }
            if chars.peek() == Some(&'"') {
                word.extend(std::iter::repeat_n('\\', count / 2));
                if count % 2 == 1 {
                    chars.next();
                    word.push('"');
                }
            } else {
                word.extend(std::iter::repeat_n('\\', count));
            }
            started = true;
        } else if character == '"' {
            quoted = !quoted;
            started = true;
        } else if character.is_whitespace() && !quoted {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(character);
            started = true;
        }
    }
    if started {
        words.push(word);
    }
    (!quoted).then_some(words)
}
