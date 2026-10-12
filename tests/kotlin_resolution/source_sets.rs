use super::*;

const KMP_BUILD: &str = r#"plugins {
    kotlin("multiplatform") version "2.0.0"
}
kotlin {
    sourceSets {
        val commonMain by getting
        val intermediateMain by creating { dependsOn(commonMain) }
        val androidMain by getting { dependsOn(intermediateMain) }
        val iosMain by creating { dependsOn(commonMain) }
    }
}
"#;

#[test]
fn expect_actual_symbols_are_selected_for_each_platform_source_set() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("kmp");
    let common = r#"package platform
expect fun platformId(): String
expect class Engine()
fun commonUse() = platformId()
"#;
    let intermediate = r#"package platform
fun intermediateUse() = platformId()
"#;
    let android = r#"package platform
actual fun platformId(): String = "android"
class ConcreteEngine
actual typealias Engine = ConcreteEngine
fun androidUse() = platformId()
fun createEngine(): Engine = Engine()
"#;
    let ios = r#"package platform
actual fun platformId(): String = "ios"
fun iosUse() = platformId()
"#;
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"kmp\"\n",
    );
    write(&root.join("build.gradle.kts"), KMP_BUILD);
    write(
        &root.join("src/commonMain/kotlin/platform/Platform.kt"),
        common,
    );
    write(
        &root.join("src/intermediateMain/kotlin/platform/Intermediate.kt"),
        intermediate,
    );
    write(
        &root.join("src/androidMain/kotlin/platform/Android.kt"),
        android,
    );
    write(&root.join("src/iosMain/kotlin/platform/Ios.kt"), ios);

    let project = report(&run(&root, "repo", None));
    let common_file = file(&project, "/commonMain/kotlin/platform/Platform.kt");
    let intermediate_file = file(
        &project,
        "/intermediateMain/kotlin/platform/Intermediate.kt",
    );
    let android_file = file(&project, "/androidMain/kotlin/platform/Android.kt");
    let ios_file = file(&project, "/iosMain/kotlin/platform/Ios.kt");

    assert!(
        !common_file["functions"]
            .as_array()
            .expect("common executable scopes")
            .iter()
            .any(|item| item["name"] == "Engine"),
        "expect class declarations have no executable primary-constructor baseline"
    );

    exact_target(
        ref_at(common_file, common, "platformId()", 1),
        "/commonMain/kotlin/platform/Platform.kt",
        "platformId",
    );
    exact_target(
        ref_at(intermediate_file, intermediate, "platformId()", 0),
        "/commonMain/kotlin/platform/Platform.kt",
        "platformId",
    );
    exact_target(
        ref_at(android_file, android, "platformId()", 1),
        "/androidMain/kotlin/platform/Android.kt",
        "platformId",
    );
    exact_target(
        ref_at(ios_file, ios, "platformId()", 1),
        "/iosMain/kotlin/platform/Ios.kt",
        "platformId",
    );

    let engine_alias = ref_at(android_file, android, "ConcreteEngine", 1);
    exact_target(
        engine_alias,
        "/androidMain/kotlin/platform/Android.kt",
        "ConcreteEngine",
    );
    let engine_call = ref_at(android_file, android, "Engine()", 1);
    exact_target(
        engine_call,
        "/androidMain/kotlin/platform/Android.kt",
        "Engine",
    );
}

#[test]
fn candidate_rebuilds_kotlin_source_set_visibility_for_build_script_only_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("kmp");
    let build_path = root.join("build.gradle.kts");
    let middle = "package mid\nfun middle() = 7\n";
    let client = "package client\nimport mid.middle\nfun callMiddle() = middle()\n";
    let after_build = KMP_BUILD.replace(
        "val androidMain by getting { dependsOn(intermediateMain) }",
        "val androidMain by getting",
    );
    write(
        &root.join("settings.gradle.kts"),
        "rootProject.name = \"kmp\"\n",
    );
    write(&build_path, KMP_BUILD);
    write(
        &root.join("src/intermediateMain/kotlin/mid/Middle.kt"),
        middle,
    );
    write(
        &root.join("src/androidMain/kotlin/client/Client.kt"),
        client,
    );
    let before = report(&run(&root, "repo", None));
    exact_target(
        ref_at(file(&before, "/client/Client.kt"), client, "middle()", 0),
        "/intermediateMain/kotlin/mid/Middle.kt",
        "middle",
    );

    let diff = temp.path().join("build-only.diff");
    write(
        &diff,
        concat!(
            "--- a/build.gradle.kts\n",
            "+++ b/build.gradle.kts\n",
            "@@ -5,6 +5,6 @@\n",
            "     sourceSets {\n",
            "         val commonMain by getting\n",
            "         val intermediateMain by creating { dependsOn(commonMain) }\n",
            "-        val androidMain by getting { dependsOn(intermediateMain) }\n",
            "+        val androidMain by getting\n",
            "         val iosMain by creating { dependsOn(commonMain) }\n",
            "     }\n",
        ),
    );
    let candidate = report(&run(&root, "candidate", Some(&diff)));
    assert_eq!(std::fs::read_to_string(&build_path).unwrap(), KMP_BUILD);
    let after_reference = ref_at(file(&candidate, "/client/Client.kt"), client, "middle()", 0);
    assert_ne!(after_reference["resolution"]["status"], "exact");

    write(&build_path, after_build.as_bytes());
    let fresh = report(&run(&root, "repo", None));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn internal_members_are_visible_within_a_gradle_module_only() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("multi-module");
    let settings = "rootProject.name = \"sample\"\ninclude(\":lib\", \":app\")\n";
    write(&root.join("settings.gradle.kts"), settings);
    write(
        &root.join("lib/build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
    );
    write(
        &root.join("app/build.gradle.kts"),
        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\ndependencies { implementation(project(\":lib\")) }\n",
    );
    let library = "package shared\ninternal fun internalEntry() = 1\nfun publicEntry() = 2\n";
    let same_module =
        "package shared\nimport shared.internalEntry\nfun inside() = internalEntry()\n";
    let app = "package client\nimport shared.internalEntry\nimport shared.publicEntry\nfun outside() = internalEntry() + publicEntry()\n";
    write(&root.join("lib/src/main/kotlin/shared/Library.kt"), library);
    write(
        &root.join("lib/src/main/kotlin/shared/Inside.kt"),
        same_module,
    );
    write(&root.join("app/src/main/kotlin/client/Client.kt"), app);

    let project = report(&run(&root, "repo", None));
    exact_target(
        ref_at(
            file(&project, "/lib/src/main/kotlin/shared/Inside.kt"),
            same_module,
            "internalEntry()",
            0,
        ),
        "/lib/src/main/kotlin/shared/Library.kt",
        "internalEntry",
    );
    let caller = file(&project, "/app/src/main/kotlin/client/Client.kt");
    let private_access = ref_at(caller, app, "internalEntry()", 0);
    assert_ne!(private_access["resolution"]["status"], "exact");
    exact_target(
        ref_at(caller, app, "publicEntry()", 0),
        "/lib/src/main/kotlin/shared/Library.kt",
        "publicEntry",
    );
}
