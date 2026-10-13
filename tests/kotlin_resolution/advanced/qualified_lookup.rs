use super::*;

#[test]
fn local_qualified_prefix_shadows_a_package_and_imported_top_level_name() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let client = concat!(
        "package client\n",
        "import api.item\n",
        "fun call() {\n",
        "  val api = object {}\n",
        "  api.item()\n",
        "}\n",
    );
    write(
        &root.join("src/main/kotlin/api/Api.kt"),
        "package api\nfun item() = 1\n",
    );
    write(&root.join("src/main/kotlin/client/Client.kt"), client);

    let project = report(&run(&root, "repo", None));
    let reference = ref_at(file(&project, "/client/Client.kt"), client, "item()", 0);
    assert_ne!(
        reference["resolution"]["status"], "exact",
        "the local receiver `api` must prevent treating `api.item` as a package path"
    );
}

#[test]
fn typed_receiver_identity_prefers_members_and_imported_extensions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let left_client = concat!(
        "package client\n",
        "import left.Model\n",
        "import left.render\n",
        "fun memberWins(value: Model) = value.render()\n",
        "fun hiddenExtension(value: Model) = value.extra()\n",
    );
    let right_client = concat!(
        "package client\n",
        "import right.Model\n",
        "import right.render\n",
        "fun rightReceiver(value: Model) = value.render()\n",
    );
    write(
        &root.join("src/main/kotlin/left/Model.kt"),
        "package left\nclass Model { fun render() = 1 }\n",
    );
    write(
        &root.join("src/main/kotlin/left/Extensions.kt"),
        "package left\nfun Model.render() = 2\nfun Model.extra() = 3\n",
    );
    write(
        &root.join("src/main/kotlin/right/Model.kt"),
        "package right\nclass Model\n",
    );
    write(
        &root.join("src/main/kotlin/right/Extensions.kt"),
        "package right\nfun Model.render() = 4\n",
    );
    write(&root.join("src/main/kotlin/client/Left.kt"), left_client);
    write(&root.join("src/main/kotlin/client/Right.kt"), right_client);

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/client/Left.kt"),
            left_client,
            "render()",
            0,
        ),
        "/left/Model.kt",
        "render",
    );
    assert_ne!(
        ref_at(file(&project, "/client/Left.kt"), left_client, "extra()", 0,)["resolution"]["status"],
        "exact",
        "extensions without an import must not be considered"
    );
    exact_target(
        ref_at(
            file(&project, "/client/Right.kt"),
            right_client,
            "render()",
            0,
        ),
        "/right/Extensions.kt",
        "render",
    );
}

#[test]
fn instance_extension_and_class_qualified_companion_calls_select_distinct_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package precedence\n",
        "class C { companion object { fun work() = 1 } }\n",
        "fun C.work() = 2\n",
        "fun use(value: C) = value.work()\n",
        "fun classUse() = C.work()\n",
    );
    write(&root.join("src/main/kotlin/Selection.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Selection.kt");
    let companion_start = source.find("fun work() = 1").unwrap();
    let extension_start = source.find("fun C.work()").unwrap();
    let instance_call = ref_at(file, source, "work()", 2);
    exact_target(instance_call, "/Selection.kt", "work");
    assert_eq!(
        instance_call["resolution"]["symbols"]["start_byte"], extension_start,
        "instance syntax must select the extension, not a companion member"
    );
    let class_call = ref_at(file, source, "work()", 3);
    exact_target(class_call, "/Selection.kt", "work");
    assert_eq!(
        class_call["resolution"]["symbols"]["start_byte"], companion_start,
        "class-qualified syntax must select the companion member"
    );
}

#[test]
fn local_callable_alias_companion_and_super_calls_preserve_static_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let source = concat!(
        "package sample\n",
        "fun target() = 1\n",
        "open class Base { fun inherited() = 2 }\n",
        "class Derived : Base() { fun viaSuper() = super.inherited() }\n",
        "class Registry{companion object{fun create()=Registry()}}\n",
        "fun viaCompanion() = Registry.create()\n",
        "fun viaAlias() { val alias = ::target; alias() }\n",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\n",
    );
    write(&root.join("src/main/kotlin/Sample.kt"), source);

    let project = report(&run(&root, "repo", None));
    let file = file(&project, "/Sample.kt");
    exact_target(
        ref_at(file, source, "inherited()", 1),
        "/Sample.kt",
        "inherited",
    );
    exact_target(ref_at(file, source, "create()", 1), "/Sample.kt", "create");
    exact_target(ref_at(file, source, "alias()", 0), "/Sample.kt", "target");
}

#[test]
fn compact_companion_string_interpolation_preserves_the_enclosing_type() {
    let temp = tempfile::tempdir().unwrap();
    let source = concat!(
        "package recovered\n",
        "class Messages{companion object{fun render()=\"value ${if (true) { 1 } else { 2 }}\"}}\n",
        "fun use()=Messages.render()\n",
    );
    write(&temp.path().join("Messages.kt"), source);
    let project = report(&run(temp.path(), "repo", None));
    let file = file(&project, "/Messages.kt");
    exact_target(
        ref_at(file, source, "render()", 1),
        "/Messages.kt",
        "render",
    );
    assert!(
        file["functions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|function| { !function["name"].as_str().unwrap().starts_with("script@") })
    );
}
