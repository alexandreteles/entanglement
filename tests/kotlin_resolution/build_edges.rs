use super::*;

struct VisibilityCase {
    name: &'static str,
    settings: (&'static str, &'static str),
    builds: &'static [(&'static str, &'static str)],
    declarations: &'static [(&'static str, &'static str)],
    calls: &'static [(&'static str, &'static str, Option<&'static str>)],
}

fn assert_visibility(case: VisibilityCase) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(case.name);
    write(&root.join(case.settings.0), case.settings.1);
    for (path, source) in case.builds {
        write(&root.join(path), source);
    }
    for (path, name) in case.declarations {
        write(
            &root.join(path),
            &format!("package shared\nfun {name}() = 1\n"),
        );
    }
    for (path, name, _) in case.calls {
        write(
            &root.join(path),
            &format!("package app\nimport shared.{name}\nfun use() = {name}()\n"),
        );
    }
    let project = report(&run(&root, "repo", None));
    for (path, name, target) in case.calls {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        let needle = format!("{name}()");
        let reference = ref_at(file(&project, path), &source, &needle, 0);
        if let Some(target) = target {
            exact_target(reference, target, name);
        } else {
            assert_ne!(
                reference["resolution"]["status"], "exact",
                "{}: {path}: {reference:#}",
                case.name
            );
        }
    }
}

#[test]
fn gradle_build_edges_preserve_module_and_source_set_visibility() {
    for case in [
        VisibilityCase {
            name: "gradle-layout",
            settings: (
                "settings.gradle.kts",
                r#"
        rootProject.name = "gradle-layout"
        include(":app", ":lib")
        project(":lib").projectDir = file("modules/shared-lib")
        // include(":comment-only")
        val example = "include(\":string-only\")"
    "#,
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
                    r#"
        dependencies {
            testImplementation(project(":lib"))
        }
    "#,
                ),
                ("modules/shared-lib/build.gradle.kts", ""),
            ],
            declarations: &[(
                "modules/shared-lib/src/main/kotlin/shared/Library.kt",
                "exported",
            )],
            calls: &[
                ("app/src/main/kotlin/app/Production.kt", "exported", None),
                (
                    "app/src/test/kotlin/app/Test.kt",
                    "exported",
                    Some("modules/shared-lib/src/main/kotlin/shared/Library.kt"),
                ),
            ],
        },
        VisibilityCase {
            name: "source-set-edges",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"source-set-edges\"\n",
            ),
            builds: &[(
                "build.gradle.kts",
                r#"
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
    "#,
            )],
            declarations: &[
                ("src/sharedMain/kotlin/source/Shared.kt", "sharedEntry"),
                ("src/fakeMain/kotlin/source/Fake.kt", "fakeEntry"),
            ],
            calls: &[(
                "src/androidMain/kotlin/source/Android.kt",
                "sharedEntry",
                Some("src/sharedMain/kotlin/source/Shared.kt"),
            )],
        },
        VisibilityCase {
            name: "groovy-build",
            settings: (
                "settings.gradle",
                "rootProject.name = 'groovy-build'\ninclude ':app', ':lib'\nproject(':lib').projectDir = file('modules/shared-lib')\n// include ':fake'\n",
            ),
            builds: &[
                (
                    "app/build.gradle",
                    "dependencies {\n  implementation project(':lib')\n}\ndef sample = 'implementation project(\':fake\')'\n",
                ),
                ("modules/shared-lib/build.gradle", ""),
            ],
            declarations: &[(
                "modules/shared-lib/src/main/kotlin/shared/Library.kt",
                "exported",
            )],
            calls: &[(
                "app/src/main/kotlin/app/App.kt",
                "exported",
                Some("modules/shared-lib/src/main/kotlin/shared/Library.kt"),
            )],
        },
        VisibilityCase {
            name: "variant-dependencies",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"variant-dependencies\"\ninclude(\":app\", \":lib\")\n",
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
                    "dependencies { debugImplementation(project(\":lib\")) }\n",
                ),
                ("lib/build.gradle.kts", ""),
            ],
            declarations: &[("lib/src/main/kotlin/shared/Library.kt", "exported")],
            calls: &[
                (
                    "app/src/debug/kotlin/app/Debug.kt",
                    "exported",
                    Some("lib/src/main/kotlin/shared/Library.kt"),
                ),
                ("app/src/release/kotlin/app/Release.kt", "exported", None),
            ],
        },
        VisibilityCase {
            name: "production-vs-test",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"production-vs-test\"\ninclude(\":app\", \":lib\")\n",
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
                    "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\ndependencies { implementation(project(\":lib\")) }\n",
                ),
                (
                    "lib/build.gradle.kts",
                    "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
                ),
            ],
            declarations: &[(
                "lib/src/commonTest/kotlin/shared/TestOnly.kt",
                "testOnlyEntry",
            )],
            calls: &[(
                "app/src/commonMain/kotlin/app/Production.kt",
                "testOnlyEntry",
                None,
            )],
        },
        VisibilityCase {
            name: "non-compile-dependencies",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"non-compile-dependencies\"\ninclude(\":app\", \":falseLib\", \":runtimeLib\")\n",
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
                    concat!(
                        "plugins { kotlin(\"jvm\") version \"2.0.0\" }\n",
                        "if (false) { dependencies { implementation(project(\":falseLib\")) } }\n",
                        "dependencies { runtimeOnly(project(\":runtimeLib\")) }\n",
                    ),
                ),
                ("falseLib/build.gradle.kts", ""),
                ("runtimeLib/build.gradle.kts", ""),
            ],
            declarations: &[
                ("falseLib/src/main/kotlin/hidden/Api.kt", "falseOnly"),
                (
                    "runtimeLib/src/main/kotlin/hidden/Api.kt",
                    "runtimeOnlyEntry",
                ),
            ],
            calls: &[
                ("app/src/main/kotlin/app/FalseUse.kt", "falseOnly", None),
                (
                    "app/src/main/kotlin/app/RuntimeUse.kt",
                    "runtimeOnlyEntry",
                    None,
                ),
            ],
        },
        VisibilityCase {
            name: "scoped-false-edges",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"scoped-false-edges\"\ninclude(\":app\", \":conditional\", \":foreign\", \":androidOnly\")\n",
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
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
                ),
                ("conditional/build.gradle.kts", ""),
                ("foreign/build.gradle.kts", ""),
                ("androidOnly/build.gradle.kts", ""),
            ],
            declarations: &[
                (
                    "conditional/src/main/kotlin/hidden/Api.kt",
                    "conditionalEntry",
                ),
                ("foreign/src/main/kotlin/hidden/Api.kt", "foreignEntry"),
                (
                    "androidOnly/src/main/kotlin/hidden/Api.kt",
                    "androidOnlyEntry",
                ),
            ],
            calls: &[
                (
                    "app/src/conditionalMain/kotlin/app/Use.kt",
                    "conditionalEntry",
                    None,
                ),
                (
                    "app/src/foreignMain/kotlin/app/Use.kt",
                    "foreignEntry",
                    None,
                ),
                (
                    "app/src/commonTest/kotlin/app/Use.kt",
                    "androidOnlyEntry",
                    None,
                ),
            ],
        },
        VisibilityCase {
            name: "transitive-configurations",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"transitive-configurations\"\ninclude(\":app\", \":middle\", \":apiLib\", \":implLib\")\n",
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
                    "dependencies { implementation(project(\":middle\")) }\n",
                ),
                (
                    "middle/build.gradle.kts",
                    "dependencies {\n    api(project(\":apiLib\"))\n    implementation(project(\":implLib\"))\n}\n",
                ),
                ("apiLib/build.gradle.kts", ""),
                ("implLib/build.gradle.kts", ""),
            ],
            declarations: &[
                ("apiLib/src/main/kotlin/shared/Api.kt", "apiVisible"),
                (
                    "implLib/src/main/kotlin/shared/Implementation.kt",
                    "implementationHidden",
                ),
            ],
            calls: &[
                (
                    "app/src/main/kotlin/app/ApiUse.kt",
                    "apiVisible",
                    Some("apiLib/src/main/kotlin/shared/Api.kt"),
                ),
                (
                    "app/src/main/kotlin/app/ImplementationUse.kt",
                    "implementationHidden",
                    None,
                ),
            ],
        },
        VisibilityCase {
            name: "scoped-mpp-dependency",
            settings: (
                "settings.gradle.kts",
                "rootProject.name = \"scoped-mpp-dependency\"\ninclude(\":app\", \":shared\")\n",
            ),
            builds: &[
                (
                    "app/build.gradle.kts",
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
                ),
                (
                    "shared/build.gradle.kts",
                    "plugins { kotlin(\"multiplatform\") version \"2.0.0\" }\n",
                ),
            ],
            declarations: &[("shared/src/commonMain/kotlin/shared/Api.kt", "commonEntry")],
            calls: &[
                (
                    "app/src/commonMain/kotlin/app/Common.kt",
                    "commonEntry",
                    Some("shared/src/commonMain/kotlin/shared/Api.kt"),
                ),
                (
                    "app/src/unrelatedMain/kotlin/app/Unrelated.kt",
                    "commonEntry",
                    None,
                ),
            ],
        },
    ] {
        assert_visibility(case);
    }
}
