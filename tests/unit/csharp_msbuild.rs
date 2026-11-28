use super::*;
use std::process::Command;

/// Compare source membership for every configured target against an installed .NET SDK.
#[test]
#[ignore = "Set CSHARP_CORPUS and DOTNET to run the MSBuild comparison"]
fn compile_membership_matches_msbuild() {
    let root = std::env::var_os("CSHARP_CORPUS")
        .map(PathBuf::from)
        .expect("Set CSHARP_CORPUS");
    let dotnet = std::env::var_os("DOTNET").unwrap_or_else(|| "dotnet".into());
    let files = crate::input::discover(&root)
        .unwrap()
        .map(Result::unwrap)
        .filter(|p| extension(p, "cs"))
        .collect::<Vec<_>>();
    let project = Project::load(&root, &files, &Snapshot::default()).unwrap();
    assert!(!project.views.is_empty());
    let mut count = 0;
    for (index, view) in project
        .views
        .iter()
        .enumerate()
        .filter(|(_, v)| v.configuration == "Debug")
    {
        let output = Command::new(&dotnet)
            .current_dir(&root)
            .arg("msbuild")
            .arg(&view.project)
            .arg(format!("-p:TargetFramework={}", view.framework))
            .arg("-p:Configuration=Debug")
            .args([
                "-getItem:Compile,ProjectReference",
                "-getProperty:TargetFramework",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            view.project.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let inventory = |name: &str| {
            report["Items"][name]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| PathBuf::from(item["FullPath"].as_str().unwrap()))
                .collect::<BTreeSet<_>>()
        };
        let evaluated = inventory("Compile");
        let expected = files
            .iter()
            .filter(|p| evaluated.contains(*p))
            .collect::<BTreeSet<_>>();
        let actual = files
            .iter()
            .filter(|p| project.owners(p).contains(&index))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual.symmetric_difference(&expected).collect::<Vec<_>>(),
            Vec::<&&PathBuf>::new(),
            "{} {}",
            view.project.display(),
            view.framework
        );
        assert_eq!(
            view.references.iter().cloned().collect::<BTreeSet<_>>(),
            inventory("ProjectReference"),
            "{} project references",
            view.project.display()
        );
        count += 1;
    }
    eprintln!(
        "Compared {count} project targets and {} discovered C# files.",
        files.len()
    );
}
