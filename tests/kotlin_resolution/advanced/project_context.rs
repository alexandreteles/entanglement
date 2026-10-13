use super::*;

#[test]
fn independent_gradle_roots_keep_same_package_candidates_separate() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let first = workspace.join("first");
    let second = workspace.join("second");
    let first_client = "package client\nimport shared.entry\nfun call() = entry()\n";
    let second_client = "package client\nimport shared.entry\nfun call() = entry()\n";

    write(
        &first.join("settings.gradle.kts"),
        "rootProject.name = \"first\"\n",
    );
    write(
        &second.join("settings.gradle.kts"),
        "rootProject.name = \"second\"\n",
    );
    write(
        &first.join("src/main/kotlin/shared/Entry.kt"),
        "package shared\ninternal fun entry() = 1\n",
    );
    write(
        &second.join("src/main/kotlin/shared/Entry.kt"),
        "package shared\nfun entry() = 2\n",
    );
    write(
        &first.join("src/main/kotlin/client/Client.kt"),
        first_client,
    );
    write(
        &second.join("src/main/kotlin/client/Client.kt"),
        second_client,
    );

    let project = report(&run(&workspace, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/first/src/main/kotlin/client/Client.kt"),
            first_client,
            "entry()",
            0,
        ),
        "/first/src/main/kotlin/shared/Entry.kt",
        "entry",
    );
    exact_target(
        ref_at(
            file(&project, "/second/src/main/kotlin/client/Client.kt"),
            second_client,
            "entry()",
            0,
        ),
        "/second/src/main/kotlin/shared/Entry.kt",
        "entry",
    );
}

#[test]
fn comments_and_strings_do_not_create_gradle_or_source_set_edges() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let app_build = concat!(
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
        "// dependencies { implementation(project(\":lib\")) }\n",
        "val decoy = \"implementation(project(\\\":lib\\\"))\"\n",
    );
    let client = "package client\nimport library.sharedEntry\nfun call() = sharedEntry()\n";
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"sample\"\ninclude(\":app\", \":lib\")\n",
    );
    write(&root.join("app/build.gradle.kts"), app_build);
    write(
        &root.join("lib/build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
    );
    write(
        &root.join("lib/src/main/kotlin/library/Api.kt"),
        "package library\nfun sharedEntry() = 1\n",
    );
    write(&root.join("app/src/main/kotlin/client/Client.kt"), client);

    let project = report(&run(&root, "repo", None));
    assert_ne!(
        ref_at(
            file(&project, "/app/src/main/kotlin/client/Client.kt"),
            client,
            "sharedEntry()",
            0,
        )["resolution"]["status"],
        "exact",
        "a quoted or commented dependency must not expose another Gradle module"
    );

    let kmp_root = temp.path().join("kmp");
    let build = concat!(
        "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
        "kotlin { sourceSets {\n",
        "  val commonMain by getting\n",
        "  val intermediateMain by creating\n",
        "  val androidMain by getting\n",
        "  // val androidMain by getting { dependsOn(intermediateMain) }\n",
        "  val decoy = \"dependsOn(intermediateMain)\"\n",
        "} }\n",
    );
    let intermediate = "package shared\nfun intermediateEntry() = 1\n";
    let android =
        "package client\nimport shared.intermediateEntry\nfun call() = intermediateEntry()\n";
    write(
        &kmp_root.join("settings.gradle.kts"),
        "rootProject.name = \"kmp\"\n",
    );
    write(&kmp_root.join("build.gradle.kts"), build);
    write(
        &kmp_root.join("src/intermediateMain/kotlin/shared/Intermediate.kt"),
        intermediate,
    );
    write(
        &kmp_root.join("src/androidMain/kotlin/client/Client.kt"),
        android,
    );
    let kmp = report(&run(&kmp_root, "repo", None));
    assert_ne!(
        ref_at(
            file(&kmp, "/androidMain/kotlin/client/Client.kt"),
            android,
            "intermediateEntry()",
            0,
        )["resolution"]["status"],
        "exact",
        "a quoted or commented dependsOn must not connect source sets"
    );
}
