use super::*;

#[test]
fn svelte_component_scope_module_bindings_snippets_and_default_imports() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    write(&root.join("package.json"), "{}\n");
    write(
        &root.join("src/Child.svelte"),
        r#"<script>
  export let label = 'child';
</script>
<p>{label}</p>
"#,
    );
    write(
        &root.join("src/App.svelte"),
        r#"<script module>
  export const shared = true;
</script>
<script>
  import Child from './Child.svelte';
  let ready = shared;
  let item = 'outer';
</script>

{#snippet row(item)}
  {#if item && ready}
    <Child label={item} />
  {/if}
{/snippet}

{#each [1] as item}
  {@render row(item)}
{/each}
"#,
    );

    let output = report(&run("repo", &root, None, Some("cc,cogc")));
    let app = file(&output, "App.svelte");
    assert_eq!(function(app, "<component>")["cyclomatic_complexity"], 2);
    assert_eq!(function(app, "row")["cyclomatic_complexity"], 3);
    assert!(has_resolution(app, "shared", "exact"));
    assert!(has_resolution(app, "ready", "exact"));
    assert!(has_resolution(app, "Child", "exact"));
    assert!(has_resolution(app, "row", "exact"));
    assert!(has_resolution(app, "item", "unresolved"));
}

#[test]
fn svelte_template_bindings_stop_at_branch_and_element_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let source = r#"<script>
  let value = 1;
  let self = 9;
  let sibling = 3;
  let fulfilled = 5;
  let rejected = 6;
  let promise = Promise.resolve(1);
</script>

{#if value}
  <span>{value}</span>
  {@const value = 2}
  <span>{value}</span>
{:else}
  <span>{value}</span>
{/if}

{#if true}
  {@const { self } = { self }}
  <span>{self}</span>
{/if}
<span>{self}</span>

<div>
  {#snippet inner()}<span>inside</span>{/snippet}
  {@render inner()}
</div>
{@render inner()}

{#await promise}
  <span>pending</span>
{:then fulfilled}
  <span>{fulfilled}</span>
{:catch rejected}
  <span>{rejected}</span>
{/await}
<span>{fulfilled}</span>
<span>{rejected}</span>

<div>
  {@const sibling = 4}
  <span>{sibling}</span>
</div>
<span>{sibling}</span>
"#;
    let report = analyze_source(temp.path(), "Scopes.svelte", source);
    let file = &report["files"][0];

    assert_eq!(
        resolution_at(file, source, "value = 2", 0)["resolution"]["status"],
        "unresolved",
        "the @const name should bind itself rather than the script value"
    );
    assert_eq!(
        resolution_at(file, source, "value}", 0)["resolution"]["status"],
        "exact",
        "the if condition should resolve before the fragment body binding starts"
    );
    assert_eq!(
        resolution_at(file, source, "{value}", 0)["resolution"]["status"],
        "unresolved",
        "the @const binding should shadow earlier sibling expressions in its fragment"
    );
    assert_eq!(
        resolution_at(file, source, "{value}", 1)["resolution"]["status"],
        "unresolved",
        "the @const binding should shadow the script value after its declaration"
    );
    assert_eq!(
        resolution_at(file, source, "{value}", 2)["resolution"]["status"],
        "exact",
        "the else body should see the script value rather than the if-body @const"
    );
    assert_eq!(
        resolution_at(file, source, "self }", 1)["resolution"]["status"],
        "unresolved",
        "a destructuring initializer should not resolve its own binding"
    );
    assert_eq!(
        resolution_at(file, source, "{self}", 0)["resolution"]["status"],
        "unresolved",
        "the destructured binding should shadow the script value inside its fragment"
    );
    assert_eq!(
        resolution_at(file, source, "{self}", 1)["resolution"]["status"],
        "exact",
        "the destructured binding should stop at its fragment boundary"
    );
    assert_eq!(
        resolution_at(file, source, "inner()", 1)["resolution"]["status"],
        "exact",
        "a snippet should be visible inside its containing element"
    );
    assert_eq!(
        resolution_at(file, source, "inner()", 2)["resolution"]["status"],
        "unresolved",
        "a snippet declared inside an element should not escape to siblings"
    );
    assert_eq!(
        resolution_at(file, source, "sibling = 4", 0)["resolution"]["status"],
        "unresolved",
        "the element @const name should bind itself rather than the script sibling"
    );
    assert_eq!(
        resolution_at(file, source, "{sibling}", 0)["resolution"]["status"],
        "unresolved",
        "the element @const should shadow the script sibling binding inside the element"
    );
    assert_eq!(
        resolution_at(file, source, "{sibling}", 1)["resolution"]["status"],
        "exact",
        "the element @const should not shadow the script sibling binding afterwards"
    );
    assert_eq!(
        resolution_at(file, source, "{fulfilled}", 0)["resolution"]["status"],
        "unresolved",
        "the then-branch binding should shadow the script value inside its await branch"
    );
    assert_eq!(
        resolution_at(file, source, "{fulfilled}", 1)["resolution"]["status"],
        "exact",
        "the then-branch binding should end with its await branch"
    );
    assert_eq!(
        resolution_at(file, source, "{rejected}", 0)["resolution"]["status"],
        "unresolved",
        "the catch-branch binding should shadow the script value inside its await branch"
    );
    assert_eq!(
        resolution_at(file, source, "{rejected}", 1)["resolution"]["status"],
        "exact",
        "the catch-branch binding should end with its await branch"
    );
}
