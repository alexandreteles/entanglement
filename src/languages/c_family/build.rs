use crate::snapshot::Snapshot;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::flags::Settings;
#[derive(Deserialize)]
struct Entry {
    directory: PathBuf,
    file: PathBuf,
    arguments: Option<Vec<String>>,
    command: Option<String>,
}

pub(crate) type Commands = BTreeMap<PathBuf, Vec<Settings>>;

/// Read each compilation database once from the selected source snapshot.
/// Entries for the same file are independent configurations.
pub(crate) fn commands(paths: &[PathBuf], snapshot: &Snapshot) -> crate::Result<Commands> {
    let databases = if let Some(path) = &snapshot.compile_commands {
        BTreeSet::from([path.clone()])
    } else {
        let mut checked = BTreeMap::new();
        let mut found = BTreeSet::new();
        for path in paths {
            for dir in path.ancestors().skip(1) {
                let found_here = checked.entry(dir.to_owned()).or_insert_with(|| {
                    [
                        dir.join("compile_commands.json"),
                        dir.join("build/compile_commands.json"),
                    ]
                    .into_iter()
                    .find(|candidate| snapshot.read(candidate).is_ok_and(|bytes| bytes.is_some()))
                });
                if let Some(database) = found_here {
                    found.insert(database.clone());
                    break;
                }
                if dir.join(".git").exists() {
                    break;
                }
            }
        }
        found
    };
    let mut commands = BTreeMap::new();
    for database in databases {
        let bytes = snapshot
            .read(&database)?
            .ok_or_else(|| format!("Compilation database is absent: {}", database.display()))?;
        let entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "Invalid compilation database {}: {error}",
                database.display()
            )
        })?;
        for entry in entries {
            let directory = database
                .parent()
                .unwrap_or(Path::new("."))
                .join(entry.directory);
            let file = crate::input::path_identity(&directory.join(entry.file))?;
            let args = match (entry.arguments, entry.command) {
                (Some(arguments), _) => arguments,
                (_, Some(command)) => super::flags::split_command(&command)
                    .ok_or_else(|| format!("Invalid compile command for {}", file.display()))?,
                _ => return Err(format!("Missing compile command for {}", file.display()).into()),
            };
            commands
                .entry(file.clone())
                .or_insert_with(Vec::new)
                .push(Settings::from_args(&file, &directory, &args));
        }
    }
    Ok(commands)
}
