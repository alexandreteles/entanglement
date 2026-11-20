use super::*;

/// Identify library declarations supplied by an active standard header.
pub(super) fn headers(project: &Project) -> Vec<BTreeSet<&str>> {
    project
        .units
        .iter()
        .map(|unit| {
            unit.files
                .values()
                .flat_map(|view| view.includes.values())
                .filter(|include| include.system && include.active == Active::Yes)
                .map(|include| {
                    include
                        .spelling
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                })
                .collect()
        })
        .collect()
}

pub(super) fn known(path: &[String], headers: Option<&BTreeSet<&str>>) -> bool {
    let Some(headers) = headers else { return false };
    let Some(name) = path.last().map(String::as_str) else {
        return false;
    };
    if path.len() > 1 {
        if path.first().is_none_or(|part| part != "std") {
            return false;
        }
        if path
            .iter()
            .skip(1)
            .any(|part| headers.contains(part.split('<').next().unwrap_or(part)))
        {
            return true;
        }
    }
    let header = match name {
        "puts" | "putchar" | "printf" | "fprintf" | "sprintf" | "snprintf" | "fopen" | "fclose"
        | "fread" | "fwrite" | "fputs" | "fgets" | "scanf" | "sscanf" | "FILE" => "stdio",
        "malloc" | "calloc" | "realloc" | "free" | "abort" | "exit" | "atoi" | "qsort"
        | "bsearch" | "strtol" => "stdlib",
        "strlen" | "strcpy" | "strncpy" | "strcmp" | "strncmp" | "strchr" | "strstr" | "memcpy"
        | "memset" | "memmove" | "memcmp" => "string",
        "sqrt" | "sin" | "cos" | "pow" | "floor" | "ceil" | "fabs" => "math",
        "size_t" | "ptrdiff_t" | "max_align_t" => "stddef",
        _ => return false,
    };
    headers.contains(format!("{header}.h").as_str())
        || headers.contains(format!("c{header}").as_str())
}
