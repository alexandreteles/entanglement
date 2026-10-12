use super::*;

#[test]
fn project_dependency_context_and_static_project_directory_control_visibility() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("gradle-layout");
    let settings = r#"
        rootProject.name = "gradle-layout"
        include(":app", ":lib")
        project(":lib").projectDir = file("modules/shared-lib")
        // include(":comment-only")
        val example = "include(\":string-only\")"
    "#;
    write(&root.join("settings.gradle.kts"), settings);
    write(
        &root.join("app/build.gradle.kts"),
        r#"
        dependencies {
            testImplementation(project(":lib"))
        }
    "#,
    );
    write(&root.join("modules/shared-lib/build.gradle.kts"), "");

    let library = "package shared\nfun exported() = 1\n";
    let production = "package app\nimport shared.exported\nfun mainUse() = exported()\n";
    let test = "package app\nimport shared.exported\nfun testUse() = exported()\n";
    write(
        &root.join("modules/shared-lib/src/main/kotlin/shared/Library.kt"),
        library,
    );
    write(
        &root.join("app/src/main/kotlin/app/Production.kt"),
        production,
    );
    write(&root.join("app/src/test/kotlin/app/Test.kt"), test);

    let project = report(&run(&root, "repo", None));
    let production_reference = ref_at(
        file(&project, "/app/Production.kt"),
        production,
        "exported()",
        0,
    );
    assert_ne!(production_reference["resolution"]["status"], "exact");
    exact_target(
        ref_at(file(&project, "/app/Test.kt"), test, "exported()", 0),
        "/modules/shared-lib/src/main/kotlin/shared/Library.kt",
        "exported",
    );
}

#[test]
fn source_set_depends_on_is_bound_to_its_own_declaration_block() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source-set-edges");
    let build = r#"
        kotlin {
            sourceSets {
                val sharedMain by creating
                val androidMain by getting {
                    dependsOn(sharedMain)
                    configureSomething {
                        dependsOn(fakeMain)
                    }
                }
                val fakeMain by creating
                getByName("iosMain") {
                    dependsOn("sharedMain")
                }
                named("desktopMain") { dependsOn(sharedMain) }
                androidMain.dependsOn(sharedMain)
                unrelated { dependsOn(fakeMain) }
            }
        }
    "#;
    let common = "package source\nfun sharedEntry() = 1\n";
    let android = "package source\nimport source.sharedEntry\nfun androidUse() = sharedEntry()\n";
    let unrelated = "package source\nfun fakeEntry() = 2\n";
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"source-set-edges\"\n",
    );
    write(&root.join("build.gradle.kts"), build);
    write(&root.join("src/sharedMain/kotlin/source/Shared.kt"), common);
    write(
        &root.join("src/androidMain/kotlin/source/Android.kt"),
        android,
    );
    write(&root.join("src/fakeMain/kotlin/source/Fake.kt"), unrelated);

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/androidMain/kotlin/source/Android.kt"),
            android,
            "sharedEntry()",
            0,
        ),
        "/sharedMain/kotlin/source/Shared.kt",
        "sharedEntry",
    );
}

#[test]
fn groovy_project_edges_ignore_quoted_and_commented_examples() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("groovy-build");
    write(
        &root.join("settings.gradle"),
        "rootProject.name = 'groovy-build'\ninclude ':app', ':lib'\nproject(':lib').projectDir = file('modules/shared-lib')\n// include ':fake'\n",
    );
    write(
        &root.join("app/build.gradle"),
        "dependencies {\n  implementation project(':lib')\n}\ndef sample = 'implementation project(\':fake\')'\n",
    );
    write(&root.join("modules/shared-lib/build.gradle"), "");
    let library = "package shared\nfun exported() = 1\n";
    let app = "package app\nimport shared.exported\nfun appUse() = exported()\n";
    write(
        &root.join("modules/shared-lib/src/main/kotlin/shared/Library.kt"),
        library,
    );
    write(&root.join("app/src/main/kotlin/app/App.kt"), app);

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(file(&project, "/app/App.kt"), app, "exported()", 0),
        "/modules/shared-lib/src/main/kotlin/shared/Library.kt",
        "exported",
    );
}

#[test]
fn variant_dependency_is_visible_only_to_its_matching_source_set() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("variant-dependencies");
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"variant-dependencies\"\ninclude(\":app\", \":lib\")\n",
    );
    write(
        &root.join("app/build.gradle.kts"),
        "dependencies { debugImplementation(project(\":lib\")) }\n",
    );
    write(&root.join("lib/build.gradle.kts"), "");
    let library = "package shared\nfun exported() = 1\n";
    let debug = "package app\nimport shared.exported\nfun debugUse() = exported()\n";
    let release = "package app\nimport shared.exported\nfun releaseUse() = exported()\n";
    write(&root.join("lib/src/main/kotlin/shared/Library.kt"), library);
    write(&root.join("app/src/debug/kotlin/app/Debug.kt"), debug);
    write(&root.join("app/src/release/kotlin/app/Release.kt"), release);

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/debug/kotlin/app/Debug.kt"),
            debug,
            "exported()",
            0,
        ),
        "/lib/src/main/kotlin/shared/Library.kt",
        "exported",
    );
    let release_reference = ref_at(
        file(&project, "/release/kotlin/app/Release.kt"),
        release,
        "exported()",
        0,
    );
    assert_ne!(release_reference["resolution"]["status"], "exact");
}

#[test]
fn production_source_sets_never_resolve_to_a_dependency_test_source_set() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("production-vs-test");
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"production-vs-test\"\ninclude(\":app\", \":lib\")\n",
    );
    write(
        &root.join("app/build.gradle.kts"),
        "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\ndependencies { implementation(project(\":lib\")) }\n",
    );
    write(
        &root.join("lib/build.gradle.kts"),
        "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
    );
    let library_test = "package shared\nfun testOnlyEntry() = 1\n";
    let production =
        "package app\nimport shared.testOnlyEntry\nfun productionUse() = testOnlyEntry()\n";
    write(
        &root.join("lib/src/commonTest/kotlin/shared/TestOnly.kt"),
        library_test,
    );
    write(
        &root.join("app/src/commonMain/kotlin/app/Production.kt"),
        production,
    );

    let project = report(&run(&root, "repo", None));
    let reference = ref_at(
        file(&project, "/app/src/commonMain/kotlin/app/Production.kt"),
        production,
        "testOnlyEntry()",
        0,
    );
    assert_ne!(
        reference["resolution"]["status"], "exact",
        "app commonMain must not see a same-named declaration only in lib commonTest"
    );
}

#[test]
fn false_condition_and_runtime_only_dependencies_do_not_supply_compile_symbols() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("non-compile-dependencies");
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"non-compile-dependencies\"\ninclude(\":app\", \":falseLib\", \":runtimeLib\")\n",
    );
    write(
        &root.join("app/build.gradle.kts"),
        concat!(
            "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
            "if (false) { dependencies { implementation(project(\":falseLib\")) } }\n",
            "dependencies { runtimeOnly(project(\":runtimeLib\")) }\n",
        ),
    );
    for module in ["falseLib", "runtimeLib"] {
        write(&root.join(format!("{module}/build.gradle.kts")), "");
    }
    let false_api = "package hidden\nfun falseOnly() = 1\n";
    let runtime_api = "package hidden\nfun runtimeOnlyEntry() = 2\n";
    let client = concat!(
        "package app\n",
        "import hidden.falseOnly\n",
        "import hidden.runtimeOnlyEntry\n",
        "fun compileUse() = falseOnly() + runtimeOnlyEntry()\n",
    );
    write(
        &root.join("falseLib/src/main/kotlin/hidden/Api.kt"),
        false_api,
    );
    write(
        &root.join("runtimeLib/src/main/kotlin/hidden/Api.kt"),
        runtime_api,
    );
    write(&root.join("app/src/main/kotlin/app/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/app/src/main/kotlin/app/Use.kt");
    for name in ["falseOnly()", "runtimeOnlyEntry()"] {
        assert_ne!(
            ref_at(use_file, client, name, 0)["resolution"]["status"],
            "exact",
            "{name} is unavailable to compilation because its dependency is conditional or runtime-only"
        );
    }
}

#[test]
fn conditional_foreign_and_android_test_edges_do_not_leak_into_other_source_sets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("scoped-false-edges");
    write(
        &root.join("settings.gradle.kts"),
        concat!(
            "rootProject.name = \"scoped-false-edges\"\n",
            "include(\":app\", \":conditional\", \":foreign\", \":androidOnly\")\n",
        ),
    );
    write(
        &root.join("app/build.gradle.kts"),
        concat!(
            "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
            "kotlin { sourceSets {\n",
            "  if (false) {\n",
            "    val conditionalMain by getting {\n",
            "      dependencies { implementation(project(\":conditional\")) }\n",
            "    }\n",
            "  }\n",
            "} }\n",
            "foreign.sourceSets {\n",
            "  val foreignMain by getting {\n",
            "    dependencies { implementation(project(\":foreign\")) }\n",
            "  }\n",
            "}\n",
            "dependencies { androidTestImplementation(project(\":androidOnly\")) }\n",
        ),
    );
    for module in ["conditional", "foreign", "androidOnly"] {
        write(&root.join(format!("{module}/build.gradle.kts")), "");
    }
    let conditional_api = "package hidden\nfun conditionalEntry() = 1\n";
    let foreign_api = "package hidden\nfun foreignEntry() = 2\n";
    let android_api = "package hidden\nfun androidOnlyEntry() = 3\n";
    let conditional_client =
        "package app\nimport hidden.conditionalEntry\nfun use() = conditionalEntry()\n";
    let foreign_client = "package app\nimport hidden.foreignEntry\nfun use() = foreignEntry()\n";
    let common_test_client =
        "package app\nimport hidden.androidOnlyEntry\nfun use() = androidOnlyEntry()\n";
    write(
        &root.join("conditional/src/main/kotlin/hidden/Api.kt"),
        conditional_api,
    );
    write(
        &root.join("foreign/src/main/kotlin/hidden/Api.kt"),
        foreign_api,
    );
    write(
        &root.join("androidOnly/src/main/kotlin/hidden/Api.kt"),
        android_api,
    );
    write(
        &root.join("app/src/conditionalMain/kotlin/app/Use.kt"),
        conditional_client,
    );
    write(
        &root.join("app/src/foreignMain/kotlin/app/Use.kt"),
        foreign_client,
    );
    write(
        &root.join("app/src/commonTest/kotlin/app/Use.kt"),
        common_test_client,
    );

    let project = report(&run(&root, "repo", None));
    for (path, source, needle) in [
        (
            "/app/src/conditionalMain/kotlin/app/Use.kt",
            conditional_client,
            "conditionalEntry()",
        ),
        (
            "/app/src/foreignMain/kotlin/app/Use.kt",
            foreign_client,
            "foreignEntry()",
        ),
        (
            "/app/src/commonTest/kotlin/app/Use.kt",
            common_test_client,
            "androidOnlyEntry()",
        ),
    ] {
        let reference = ref_at(file(&project, path), source, needle, 0);
        assert_ne!(
            reference["resolution"]["status"], "exact",
            "a conditional, foreign-receiver, or androidTest edge must not leak into {path}: {reference:#}"
        );
    }
}

#[test]
fn transitive_api_is_visible_but_transitive_implementation_is_hidden() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("transitive-configurations");
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"transitive-configurations\"\ninclude(\":app\", \":middle\", \":apiLib\", \":implLib\")\n",
    );
    write(
        &root.join("app/build.gradle.kts"),
        "dependencies { implementation(project(\":middle\")) }\n",
    );
    write(
        &root.join("middle/build.gradle.kts"),
        concat!(
            "dependencies {\n",
            "    api(project(\":apiLib\"))\n",
            "    implementation(project(\":implLib\"))\n",
            "}\n",
        ),
    );
    for module in ["apiLib", "implLib"] {
        write(&root.join(format!("{module}/build.gradle.kts")), "");
    }
    let api = "package shared\nfun apiVisible() = 1\n";
    let implementation = "package shared\nfun implementationHidden() = 2\n";
    let client = concat!(
        "package app\n",
        "import shared.apiVisible\n",
        "import shared.implementationHidden\n",
        "fun transitiveUse() = apiVisible() + implementationHidden()\n",
    );
    write(&root.join("apiLib/src/main/kotlin/shared/Api.kt"), api);
    write(
        &root.join("implLib/src/main/kotlin/shared/Implementation.kt"),
        implementation,
    );
    write(&root.join("app/src/main/kotlin/app/Use.kt"), client);

    let project = report(&run(&root, "repo", None));
    let use_file = file(&project, "/app/src/main/kotlin/app/Use.kt");
    exact_target(
        ref_at(use_file, client, "apiVisible()", 0),
        "/apiLib/src/main/kotlin/shared/Api.kt",
        "apiVisible",
    );
    assert_ne!(
        ref_at(use_file, client, "implementationHidden()", 0)["resolution"]["status"],
        "exact",
        "an intermediate module's implementation dependency is not re-exported"
    );
}

#[test]
fn multiplatform_source_set_dependencies_are_limited_to_the_declaring_source_set() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("scoped-mpp-dependency");
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"scoped-mpp-dependency\"\ninclude(\":app\", \":shared\")\n",
    );
    write(
        &root.join("app/build.gradle.kts"),
        concat!(
            "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
            "kotlin {\n",
            "    sourceSets {\n",
            "        val commonMain by getting {\n",
            "            dependencies { implementation(project(\":shared\")) }\n",
            "        }\n",
            "        val unrelatedMain by creating\n",
            "    }\n",
            "}\n",
        ),
    );
    write(
        &root.join("shared/build.gradle.kts"),
        "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
    );
    let library = "package shared\nfun commonEntry() = 1\n";
    let common = "package app\nimport shared.commonEntry\nfun commonUse() = commonEntry()\n";
    let unrelated = "package app\nimport shared.commonEntry\nfun unrelatedUse() = commonEntry()\n";
    write(
        &root.join("shared/src/commonMain/kotlin/shared/Api.kt"),
        library,
    );
    write(
        &root.join("app/src/commonMain/kotlin/app/Common.kt"),
        common,
    );
    write(
        &root.join("app/src/unrelatedMain/kotlin/app/Unrelated.kt"),
        unrelated,
    );

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/app/src/commonMain/kotlin/app/Common.kt"),
            common,
            "commonEntry()",
            0,
        ),
        "/shared/src/commonMain/kotlin/shared/Api.kt",
        "commonEntry",
    );
    assert_ne!(
        ref_at(
            file(&project, "/app/src/unrelatedMain/kotlin/app/Unrelated.kt"),
            unrelated,
            "commonEntry()",
            0,
        )["resolution"]["status"],
        "exact",
        "a source-set-scoped project dependency does not leak to sibling source sets"
    );
}
