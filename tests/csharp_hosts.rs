mod support;
use support::{file, function, patch, references_to, report, run, write};

#[test]
fn scripts_follow_relative_loads_and_keep_directive_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let script = "#r \"System.Xml\"\n#load \"sub/Helpers.csx\"\nint Run() => Twice(2);\n";
    write(&root.join("Entry.csx"), script);
    write(
        &root.join("sub/Helpers.csx"),
        "#load \"../Entry.csx\"\nint Twice(int n) => n+n;\n",
    );
    write(&root.join("Other.csx"), "int Twice(int n) => 0;\n");
    let data = report(&run("repo", root, None, Some("cc,structure")));
    let main = file(&data, "Entry.csx");
    assert_eq!(main["nloc"], 3);
    assert_eq!(
        function(main, "Run")["start_byte"],
        script.find("int Run").unwrap()
    );
    assert_eq!(
        references_to(main, "Twice")[0]["resolution"]["status"],
        "exact"
    );
    let relations = data["structure"]["relationships"].as_array().unwrap();
    assert!(
        relations
            .iter()
            .any(|edge| edge["kind"] == "import" && edge["resolution"] == "external")
    );
    let diff = root.join("load.diff");
    write(
        &diff,
        patch(
            "Entry.csx",
            Some(script),
            Some(&script.replace("sub/Helpers.csx", "Missing.csx")),
        ),
    );
    let after = report(&run("candidate", root, Some(&diff), Some("cc,structure")));
    assert_ne!(
        references_to(file(&after, "Entry.csx"), "Twice")[0]["resolution"]["status"],
        "exact"
    );
}

#[test]
fn razor_keeps_guest_ranges_and_resolves_a_code_behind_member() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("Demo.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk.Razor\"/>",
    );
    let template = "@namespace Demo\n<h1>@Title</h1>\n@code {\n public int Score(bool ok) => ok ? Extra() : 0;\n}\n";
    write(&root.join("Card.razor"), template);
    write(
        &root.join("Card.razor.cs"),
        "namespace Demo; public partial class Card { int Extra() => 2; string Title => \"ok\"; }",
    );
    write(
        &root.join("Page.cshtml"),
        "<p>Hello</p>\n@functions { int Value() => 1; }\n",
    );
    let data = report(&run("repo", root, None, Some("cc,structure")));
    let card = file(&data, "Card.razor");
    assert_eq!(card["language"], "razor");
    assert_eq!(function(card, "Score")["cyclomatic_complexity"], 2);
    assert_eq!(
        function(card, "Score")["start_byte"],
        template.find("public int").unwrap()
    );
    assert_eq!(
        references_to(card, "Extra")[0]["resolution"]["status"],
        "exact"
    );
    assert_eq!(
        function(file(&data, "Page.cshtml"), "Value")["cyclomatic_complexity"],
        1
    );
    assert!(
        card["injections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["analyzed"] == true)
    );
}

#[test]
fn razor_resolves_model_directives_components_and_markup_guests() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("Portal.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk.Razor\"/>",
    );
    write(
        &root.join("Account.cs"),
        "namespace Portal; public class Account { public int Total => 1; }",
    );
    write(
        &root.join("Badge.razor"),
        "@namespace Portal\n<span>Hello</span>",
    );
    write(
        &root.join("Home.razor"),
        "@namespace Portal\n<Badge />\n<style>h1 { color: red }</style>\n<script>function twice(x) { return x+x; }</script>",
    );
    write(
        &root.join("Page.cshtml"),
        "@using Portal\n@model Account\n<p>@Model.Total</p>",
    );
    let output = report(&run("repo", root, None, Some("cc,structure")));
    let home = file(&output, "Home.razor");
    assert_eq!(function(home, "twice")["cyclomatic_complexity"], 1);
    assert_eq!(
        references_to(home, "Badge")[0]["resolution"]["status"],
        "exact"
    );
    let page = file(&output, "Page.cshtml");
    assert!(
        output["structure"]["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["path"].as_str().unwrap().ends_with("Page.cshtml"))
            .all(|row| !row["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l == "parse_error"))
    );
    for name in ["Account", "Total"] {
        assert_eq!(
            references_to(page, name)[0]["resolution"]["status"],
            "exact",
            "{name}"
        );
    }
}

#[test]
fn generic_components_keep_their_code_behind_and_render_branches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("Demo.csproj"),
        "<Project Sdk=\"Microsoft.NET.Sdk.Razor\"/>",
    );
    let template = "@namespace Demo\n@typeparam T\n@if (Visible()) { <span>@Title</span> }\n@code { public int Read() => Extra(); }";
    write(&root.join("Box.razor"), template);
    write(
        &root.join("Box.razor.cs"),
        "namespace Demo; public partial class Box<T> { bool Visible() => true; string Title => \"box\"; int Extra() => 1; }",
    );
    let data = report(&run("repo", root, None, Some("cc,structure")));
    let component = file(&data, "Box.razor");
    assert_eq!(function(component, "<render>")["cyclomatic_complexity"], 2);
    for name in ["Visible", "Title", "Extra"] {
        assert_eq!(
            references_to(component, name)[0]["resolution"]["status"],
            "exact",
            "{name}"
        );
    }
    let parts = data["structure"]["effective"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["unit"].as_str().unwrap().ends_with("::Box`1"))
        .unwrap();
    assert_eq!(parts["evidence"].as_array().unwrap().len(), 2);
}

#[test]
fn razor_project_items_control_component_ownership_in_a_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let project = "<Project Sdk=\"Microsoft.NET.Sdk.Web\"><ItemGroup><Content Remove=\"Card.razor\"/></ItemGroup></Project>";
    let included = "<Project Sdk=\"Microsoft.NET.Sdk.Web\"/>";
    write(&root.join("Demo.csproj"), project);
    write(
        &root.join("Card.razor"),
        "@namespace Demo\n@code { int Read() => Extra(); }",
    );
    write(
        &root.join("Code.cs"),
        "namespace Demo; public partial class Card { int Extra() => 1; }",
    );
    let before = report(&run("repo", root, None, Some("cc")));
    assert_ne!(
        references_to(file(&before, "Card.razor"), "Extra")[0]["resolution"]["status"],
        "exact"
    );
    let diff = root.join("content.diff");
    write(&diff, patch("Demo.csproj", Some(project), Some(included)));
    let after = report(&run("candidate", root, Some(&diff), Some("cc")));
    assert_eq!(
        references_to(file(&after, "Card.razor"), "Extra")[0]["resolution"]["status"],
        "exact"
    );
}
