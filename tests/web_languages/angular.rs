use super::*;

#[test]
fn angular_control_flow_and_bindings_use_template_syntax() {
    let temp = tempfile::tempdir().unwrap();
    let source = r##"@if (ready && enabled; as visible) {
  @for (item of items; track item.id; let i = $index) {
    <button [disabled]="!visible" (click)="pick(item, $event)">{{ i }} {{ item.name | uppercase }}</button>
  } @empty { <p>{{ item }}</p> }
} @else { <p>{{ visible }}</p> }
<p>{{ $event }}</p>
@let label = title;
<p>{{ label }}</p>
<div *ngIf="ready as active">{{ active }}</div>
<ul><li *ngFor="let row of rows; index as n">{{ row }} {{ n }}</li></ul>
<script lang="ts">function decide(flag: boolean) { return flag ? 1 : 0; }</script>
<style>p { color: red; }</style>
"##;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_angular::language())
        .unwrap();
    assert!(!parser.parse(source, None).unwrap().root_node().has_error());
    let output = analyze_source(temp.path(), "view.component.html", source);
    let view = &output["files"][0];
    assert_eq!(view["language"], "angular");
    assert_eq!(function(view, "<template>")["cyclomatic_complexity"], 6);
    assert_eq!(function(view, "<template>")["cognitive_complexity"], 7);
    assert_eq!(function(view, "decide")["cyclomatic_complexity"], 2);
    for language in ["typescript", "css"] {
        assert!(
            view["injections"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["language"] == language && item["analyzed"] == true)
        );
    }
}

#[test]
fn angular_inline_templates_require_verified_imports_and_literal_content() {
    let temp = tempfile::tempdir().unwrap();
    for extension in ["ts", "js"] {
        let source = r#"import { Component as View } from '@angular/core';
import * as ng from '@angular/core';
View({template: `@if (ready) {<p>{{ value }}</p>}`});
ng.Component({template: '<p>{{ other }}</p>'});
View({template: '<p>\\n{{ escaped }}</p>'});
View({template: `<p>${dynamic}</p>`});
function shadow(View) { View({template: '<p>{{ hidden }}</p>'}); }
function other() { const View = make(); View({template: '<p>{{ hiddenToo }}</p>'}); }
"#;
        let output = analyze_source(temp.path(), &format!("inline.{extension}"), source);
        let view = &output["files"][0];
        let injections = view["injections"].as_array().unwrap();
        let angular = injections
            .iter()
            .filter(|item| item["language"] == "angular")
            .collect::<Vec<_>>();
        assert_eq!(angular.len(), 2, "{extension}: {injections:?}");
        assert!(angular.iter().all(|item| item["analyzed"] == true));
        let first = source.find("@if").unwrap();
        assert_eq!(angular[0]["start_byte"], first);
        assert_eq!(
            angular[0]["end_byte"],
            first + "@if (ready) {<p>{{ value }}</p>}".len()
        );
        let templates = view["functions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["name"] == "<template>")
            .collect::<Vec<_>>();
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0]["cyclomatic_complexity"], 2);
        assert_eq!(templates[0]["cognitive_complexity"], 1);
        assert_eq!(templates[1]["cyclomatic_complexity"], 1);
    }
    let output = analyze_source(
        temp.path(),
        "unrelated.ts",
        "function Component(x) {} Component({template: '<p>{{ ignored }}</p>'});",
    );
    assert!(
        !output["files"][0]["injections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["language"] == "angular")
    );
}

#[test]
fn angular_alternate_branches_keep_control_flow_nesting() {
    let temp = tempfile::tempdir().unwrap();
    for (name, source, expected) in [
        (
            "chain",
            "@if (outer) { @if (inner) {} @else if (other) { @if (deep) {} } }",
            7,
        ),
        (
            "else",
            "@if (outer) { @if (inner) {} @else { @if (deep) {} } }",
            7,
        ),
        ("structural", "<p *ngIf=\"a ? b : c\"></p>", 2),
    ] {
        let output = analyze_source(temp.path(), &format!("{name}.component.html"), source);
        assert_eq!(
            function(&output["files"][0], "<template>")["cognitive_complexity"],
            expected,
            "{name}"
        );
    }
}

#[test]
fn angular_inline_patch_matches_fresh_template_ranges_and_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("inline.ts");
    let before = "import { Component } from '@angular/core'; Component({template: `@if (ready) {<p>{{ value }}</p>}`});\n";
    let after = before.replace("@if (ready)", "@for (value of values; track value)");
    write(&path, before);
    let diff = temp.path().join("inline.diff");
    write(
        &diff,
        format!("--- a/inline.ts\n+++ b/inline.ts\n@@ -1 +1 @@\n-{before}+{after}"),
    );
    let patched = report(&run("patch", &path, Some(&diff), Some("cc,cogc,halstead")));
    assert_eq!(std::fs::read(&path).unwrap(), before.as_bytes());
    let fresh = analyze_source(temp.path(), "inline.ts", &after);
    let changed = &patched["patch"]["files"][0]["after"];
    assert_eq!(
        function(changed, "<template>"),
        function(&fresh["files"][0], "<template>")
    );
    assert_eq!(changed["injections"], fresh["files"][0]["injections"]);
}
