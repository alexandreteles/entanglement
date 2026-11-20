use super::{ParsedFile, Worker};
use crate::languages::c_family::project::Project;
use crate::{Result, metrics::selection::Selection, snapshot::Snapshot};
use tree_sitter::{Node, Tree};

/// Select one physical metric view after all files and virtual build inputs exist.
pub(crate) fn prepare(
    files: &mut [ParsedFile],
    selection: Selection,
    snapshot: &mut Snapshot,
) -> Result<()> {
    let mut worker = Worker::new(selection)?;
    snapshot.c_project = Project::build(files, snapshot, |file| {
        if file
            .facts
            .path
            .extension()
            .is_some_and(|ext| ext == "h" || ext == "inc")
        {
            prefer_cpp(&mut worker, file)
        } else {
            Ok(file.language == "source.cpp")
        }
    })?;
    for file in files
        .iter_mut()
        .filter(|file| matches!(file.language.as_str(), "source.c" | "source.cpp"))
    {
        let cpp = match snapshot.c_project.dialect(&file.facts.target) {
            Some(mode) => mode,
            None if file
                .facts
                .path
                .extension()
                .is_some_and(|ext| ext == "h" || ext == "inc") =>
            {
                prefer_cpp(&mut worker, file)?
            }
            None => worker
                .select_file(&file.facts.path)?
                .is_some_and(|choice| choice.id == "source.cpp"),
        };
        let label = if cpp { "cpp" } else { "c" };
        if file.facts.analysis.language != label {
            let choice = worker
                .registry
                .select_injection(label)?
                .ok_or("Missing C-family grammar")?;
            *file = worker.analyze_selected_source(
                &file.facts.path,
                file.facts.source.to_vec(),
                Some(file),
                &[],
                choice,
            )?;
        }
        super::c_family_metrics::recover(file, &snapshot.c_project, &mut worker)?;
        super::c_family_views::collect(file, &mut snapshot.c_project, &mut worker)?;
    }
    Ok(())
}

fn prefer_cpp(worker: &mut Worker, file: &ParsedFile) -> Result<bool> {
    let tree = |worker: &mut Worker, label: &str| -> Result<Tree> {
        if file.facts.analysis.language == label {
            return Ok(file.tree.clone());
        }
        let choice = worker
            .registry
            .select_injection(label)?
            .ok_or("Missing C-family grammar")?;
        worker.parse_tree(&choice, &file.facts.source, &[], None)
    };
    let c = quality(tree(worker, "c")?.root_node());
    let cpp = quality(tree(worker, "cpp")?.root_node());
    Ok(cpp.0 < c.0 || cpp.0 == c.0 && cpp.1)
}

fn quality(root: Node<'_>) -> (usize, bool) {
    let mut errors = 0;
    let mut cpp = false;
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.is_error() || node.is_missing() {
            errors += 1;
        }
        cpp |= matches!(
            node.kind(),
            "namespace_definition"
                | "template_declaration"
                | "class_specifier"
                | "access_specifier"
                | "concept_definition"
                | "lambda_expression"
        );
        pending.extend((0..node.child_count()).filter_map(|index| node.child(index)));
    }
    (errors, cpp)
}
