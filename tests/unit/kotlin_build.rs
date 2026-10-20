use super::{DependencyExposure, DependencyScope, ProjectDependency};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn parse_includes(source: &str) -> BTreeSet<String> {
    super::parse(source, super::ScriptKind::Kotlin).0.modules
}

fn parse_project_directories(source: &str) -> BTreeMap<String, PathBuf> {
    super::parse(source, super::ScriptKind::Kotlin)
        .0
        .directories
}

fn parse_project_dependency_details(source: &str) -> BTreeSet<ProjectDependency> {
    super::parse(source, super::ScriptKind::Kotlin)
        .1
        .dependencies
}

fn parse_source_edges(source: &str, edges: &mut BTreeMap<String, BTreeSet<String>>) {
    for (child, parents) in super::parse(source, super::ScriptKind::Kotlin)
        .1
        .source_edges
    {
        edges.entry(child).or_default().extend(parents);
    }
}

#[test]
fn includes_ignore_comments_strings_dynamic_paths_and_nested_calls() {
    let source = r#"
        // include(":comment")
        val text = "include(\":string\") and https://example.test/a//b"
        include(":app", ":lib")
        if (providers.gradleProperty("extra").isPresent) {
            include(":conditional")
        }
        if (false)
            include(":unbraced-conditional")
        configureSomething {
            include(":foreign-block")
        }
        include(":$dynamic")
    "#;
    assert_eq!(
        parse_includes(source),
        BTreeSet::from([":app".into(), ":lib".into()])
    );
}

#[test]
fn dependencies_keep_configuration_context_and_ignore_fake_project_calls() {
    let source = r#"
        val example = "implementation(project(\":fake\"))"
        /* dependencies { implementation(project(":comment")) } */
        dependencies {
            implementation(project(":lib"))
            testImplementation(project(":test-support"))
            androidTestImplementation(project(":android-test-support"))
            commonTestApi(project(":common-test-support"))
            contestImplementation(project(":contest-support"))
            debugApi(project(":debug-lib"))
        api(project(":public-api"))
        compileOnly(project(":compile-only"))
        runtimeOnly(project(":runtime-only"))
            implementation(project(path = ":named-arg"))
            implementation(project(projectPath))
            // implementation(project(":comment"))
            nested { implementation(project(":nested")) }
        }
    if (false) {
        dependencies { implementation(project(":conditional")) }
    }
    if (false)
        dependencies { implementation(project(":unbraced-conditional")) }
    dependencies {
        if (false)
            implementation(project(":unbraced-configuration"))
    }
    "#;
    let parsed = parse_project_dependency_details(source);
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":lib".into(),
        configuration: "implementation".into(),
        scope: DependencyScope::Production,
        exposure: DependencyExposure::Implementation,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":test-support".into(),
        configuration: "testImplementation".into(),
        scope: DependencyScope::Test("test".into()),
        exposure: DependencyExposure::Implementation,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":android-test-support".into(),
        configuration: "androidTestImplementation".into(),
        scope: DependencyScope::Test("androidTest".into()),
        exposure: DependencyExposure::Implementation,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":common-test-support".into(),
        configuration: "commonTestApi".into(),
        scope: DependencyScope::Test("commonTest".into()),
        exposure: DependencyExposure::Api,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":contest-support".into(),
        configuration: "contestImplementation".into(),
        scope: DependencyScope::Variant("contest".into()),
        exposure: DependencyExposure::Implementation,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":debug-lib".into(),
        configuration: "debugApi".into(),
        scope: DependencyScope::Variant("debug".into()),
        exposure: DependencyExposure::Api,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":public-api".into(),
        configuration: "api".into(),
        scope: DependencyScope::Production,
        exposure: DependencyExposure::Api,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":compile-only".into(),
        configuration: "compileOnly".into(),
        scope: DependencyScope::Production,
        exposure: DependencyExposure::CompileOnly,
    }));
    assert!(parsed.contains(&super::ProjectDependency {
        path: ":runtime-only".into(),
        configuration: "runtimeOnly".into(),
        scope: DependencyScope::Production,
        exposure: DependencyExposure::Runtime,
    }));
    assert!(!parsed.iter().any(|dependency| {
        dependency.path.contains("fake")
            || dependency.path.contains("comment")
            || dependency.path.contains("nested")
            || dependency.path.contains("conditional")
    }));
    assert_eq!(
        parsed
            .iter()
            .map(|dependency| dependency.path.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ":lib".into(),
            ":test-support".into(),
            ":android-test-support".into(),
            ":common-test-support".into(),
            ":contest-support".into(),
            ":debug-lib".into(),
            ":public-api".into(),
            ":compile-only".into(),
            ":runtime-only".into(),
        ])
    );
}

#[test]
fn source_set_dependency_blocks_keep_static_scope_and_reject_unknown_ancestors() {
    let source = r#"
        kotlin {
            sourceSets {
                val commonMain by getting {
                    dependencies {
                        implementation(project(":common-implementation"))
                        api(project(":common-api"))
                    }
                }
                val androidMain by creating {
                    dependencies { implementation(project(":android-only")) }
                }
                commonMain.dependencies {
                    implementation(project(":common-dsl"))
                }
                val unrelatedMain by creating {
                    unrelatedConfiguration {
                        dependencies { implementation(project(":uncertain")) }
                    }
                    dependencies { implementation(project(":unrelated-set")) }
                }
            }
        }
        if (false) {
            dependencies { implementation(project(":conditional")) }
        }
    "#;

    let parsed = parse_project_dependency_details(source);
    let scopes = parsed
        .iter()
        .map(|dependency| (dependency.path.as_str(), &dependency.scope))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        scopes[":common-implementation"],
        &DependencyScope::Variant("commonMain".into())
    );
    assert_eq!(
        scopes[":common-api"],
        &DependencyScope::Variant("commonMain".into())
    );
    assert_eq!(
        scopes[":common-dsl"],
        &DependencyScope::Variant("commonMain".into())
    );
    assert_eq!(
        scopes[":android-only"],
        &DependencyScope::Variant("androidMain".into())
    );
    assert_eq!(
        scopes[":unrelated-set"],
        &DependencyScope::Variant("unrelatedMain".into())
    );
    assert!(!scopes.contains_key(":uncertain"));
    assert!(!scopes.contains_key(":conditional"));
}

#[test]
fn groovy_literals_and_comments_are_scanned_without_confusing_urls() {
    let settings = r#"
        // include ':comment'
        rootProject.name = 'example'
        include ':app', ':lib'
        include /:unsupported/
        def message = 'https://example.test/a//b include(\':fake\')'
    "#;
    assert_eq!(
        super::parse(settings, super::ScriptKind::Groovy).0.modules,
        BTreeSet::from([":app".into(), ":lib".into()])
    );
    let build = r#"
        dependencies {
            implementation project(':lib')
            testImplementation(project(':test-support'))
            // implementation project(':comment')
        }
    "#;
    assert_eq!(
        super::parse(build, super::ScriptKind::Groovy)
            .1
            .dependencies
            .into_iter()
            .map(|dependency| (dependency.path, dependency.exposure))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            (":lib".into(), DependencyExposure::Implementation),
            (":test-support".into(), DependencyExposure::Implementation),
        ])
    );
}

#[test]
fn source_set_edges_are_scoped_to_their_declaration_blocks() {
    let source = r#"
        kotlin {
            sourceSets {
                val sharedMain by creating
                val androidMain by getting {
                    dependsOn(sharedMain)
                    configureSomething { dependsOn(fakeMain) }
                }
                getByName("iosMain") {
                    dependsOn("sharedMain")
                }
                named("desktopMain") { dependsOn(sharedMain) }
                androidMain.dependsOn(sharedMain)
                unrelated { dependsOn(fakeMain) }
                val fakeMain by creating {
                    // dependsOn(sharedMain)
                    val text = "dependsOn(fakeMain)"
                }
                val guardedMain by creating {
                    if (false)
                        dependsOn(sharedMain)
                }
                if (false)
                    falseChild.dependsOn(sharedMain)
            }
        }
    "#;
    let mut edges = BTreeMap::new();
    parse_source_edges(source, &mut edges);
    assert_eq!(edges["androidMain"], BTreeSet::from(["sharedMain".into()]));
    assert_eq!(edges["iosMain"], BTreeSet::from(["sharedMain".into()]));
    assert_eq!(edges["desktopMain"], BTreeSet::from(["sharedMain".into()]));
    assert!(!edges.contains_key("fakeMain"));
    assert!(!edges.contains_key("guardedMain"));
    assert!(!edges.contains_key("falseChild"));
}

#[test]
fn source_set_edges_reject_conditionals_and_unrelated_dsl_blocks() {
    let source = r#"
        if (false) {
            kotlin {
                sourceSets {
                    val conditionalBase by creating
                    val conditionalChild by creating { dependsOn(conditionalBase) }
                }
            }
        }
        configureSomething {
            sourceSets {
                val foreignBase by creating
                val foreignChild by creating { dependsOn(foreignBase) }
            }
        }
        kotlin {
            sourceSets {
                val realBase by creating
                val realChild by creating { dependsOn(realBase) }
            }
        }
    "#;
    let mut edges = BTreeMap::new();
    parse_source_edges(source, &mut edges);

    assert_eq!(edges["realChild"], BTreeSet::from(["realBase".into()]));
    assert!(!edges.contains_key("conditionalChild"));
    assert!(!edges.contains_key("foreignChild"));
}

#[test]
fn project_directory_requires_a_static_relative_file_literal() {
    let settings = r#"
        include(":lib", ":other")
        project(":lib").projectDir = file("modules/library")
        project(":other").projectDir = file(modulePath)
        project(":dynamic").projectDir = file("$modulePath")
        if (false) {
            project(":conditional").projectDir = file("modules/conditional")
        }
        if (false)
            project(":unbraced-conditional").projectDir = file("modules/unbraced")
        configureSomething {
            project(":foreign").projectDir = file("modules/foreign")
        }
    "#;
    assert_eq!(
        parse_project_directories(settings),
        BTreeMap::from([(":lib".into(), "modules/library".into())])
    );
}

#[test]
fn source_set_delegates_stop_at_semicolons_on_the_same_line() {
    let source = r#"
        kotlin {
            sourceSets {
                val firstMain by creating; val childMain by creating { dependsOn(firstMain) }
            }
        }
    "#;
    let mut edges = BTreeMap::new();
    parse_source_edges(source, &mut edges);

    assert_eq!(edges["childMain"], BTreeSet::from(["firstMain".into()]));
    assert!(!edges.contains_key("firstMain"));
}

#[test]
fn unsupported_arguments_do_not_become_static_literals() {
    let (settings, build) = super::parse(
        r#"
        project(":lib", configuration = "test").projectDir = file("lib")
        project(":other").projectDir = file("other", mode = "test")
        dependencies {
            implementation(project(":lib", configuration = "test"))
            implementation(project(":valid"))
        }
        kotlin { sourceSets {
            named("childMain", extra = "value") { dependsOn(baseMain) }
            val realMain by getting {
                dependsOn(sharedMain.someValue)
                dependsOn(otherMain + extraMain)
            }
        } }
    "#,
        super::ScriptKind::Kotlin,
    );
    assert!(settings.directories.is_empty());
    assert_eq!(
        build
            .dependencies
            .iter()
            .map(|dependency| dependency.path.as_str())
            .collect::<Vec<_>>(),
        [":valid"]
    );
    assert_eq!(
        build.source_edges,
        BTreeMap::from([(
            "realMain".into(),
            BTreeSet::from(["sharedMain".into(), "otherMain".into()])
        )])
    );
}
