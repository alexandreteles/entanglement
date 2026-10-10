use super::*;

#[test]
fn svelte_scripts_styles_and_template_expressions_use_registered_analyzers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    write(
        &root.join("package.json"),
        r##"{
  "imports": {
    "#lib": "./src/lib/index.js",
    "#lib/*": "./src/lib/*",
    "#config": { "types": "./src/config.ts", "default": "./src/config.js" },
    "#vendor": "external-package",
    "#fallback": { "import": { "development": "./src/dev.ts" }, "default": "./src/config.ts" }
  }
}
"##,
    );
    write(
        &root.join("src/lib/index.ts"),
        "export function share(value: string) { return value; }\n",
    );
    write(
        &root.join("src/config.ts"),
        "export function setting(value: string) { return value; }\n",
    );
    write(
        &root.join("src/lib/format.ts"),
        "export function format(value: string) { return value.trim(); }\n",
    );
    write(
        &root.join("src/lib/player.svelte.ts"),
        "export function play(id: string) { return id; }\n",
    );
    write(
        &root.join("src/lib/Row.svelte"),
        r#"<script lang="ts">
  import { format } from './format';
  import { play } from './player.svelte';
  import { request } from '$lib/api';
  import { format as kitFormat } from '#lib/format.js';
  import { share } from '#lib';
  import { setting } from '#config';
  import { vendor } from '#vendor';
  import { missing } from '#missing';
  import { setting as fallbackSetting } from '#fallback';
  let { items, open }: { items: string[]; open: boolean } = $props();
  function pick(value: string): string {
    return open && value ? pick(format(value)) : play(request(value));
  }
  function kit(value: string) {
    return kitFormat(share(setting(fallbackSetting(vendor(missing(value))))));
  }
</script>

{#if open && items.length}
  {#each items as item (item)}
    <button onclick={() => (item ? pick(item) : null)} {@attach focus}>{item}</button>
  {/each}
{:else}
  <p>none</p>
{/if}

<style>
  p { color: red; }
</style>
"#,
    );
    write(
        &root.join("src/lib/Plain.svelte"),
        "<script>\n  let count = 0;\n</script>\n<button onclick={() => count && count++}>{count}</button>\n",
    );
    let output = report(&run("repo", &root, None, Some("cc,cogc")));

    let row = file(&output, "Row.svelte");
    assert_eq!(row["language"], "svelte");
    assert_eq!(function(row, "<component>")["cyclomatic_complexity"], 4);
    let pick = function(row, "pick");
    assert_eq!(pick["cyclomatic_complexity"], 3);
    assert!(recursive(pick));
    let handlers = row["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["name"].as_str().unwrap().starts_with("anonymous"))
        .collect::<Vec<_>>();
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0]["cyclomatic_complexity"], 2);
    let injections = row["injections"].as_array().unwrap();
    assert!(injections.iter().all(|item| item["analyzed"] == true));
    assert!(injections.iter().any(|item| item["language"] == "css"));
    assert!(has_resolution(row, "format", "exact"));
    assert!(has_resolution(row, "play", "exact"));
    assert!(has_resolution(row, "request", "unresolved"));
    assert!(!has_resolution(row, "request", "external"));
    assert!(has_resolution(row, "kitFormat", "exact"));
    assert!(has_resolution(row, "share", "exact"));
    assert!(has_resolution(row, "setting", "exact"));
    assert!(has_resolution(row, "fallbackSetting", "exact"));
    assert!(has_resolution(row, "vendor", "external"));
    assert!(has_resolution(row, "missing", "unresolved"));

    let plain = file(&output, "Plain.svelte");
    let languages = plain["injections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["language"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(languages.iter().all(|language| *language == "javascript"));
    assert_eq!(function(plain, "<component>")["cyclomatic_complexity"], 1);
    assert_eq!(function(plain, "anonymous@4")["cyclomatic_complexity"], 2);
    assert!(has_resolution(plain, "count", "exact"));
}
