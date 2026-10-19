use cargo_metadata::MetadataCommand;
use serde_json::{Value, json};
use std::{env, error::Error, fs, path::Path};
use tree_sitter_generate::{OptLevel, generate_parser_in_directory};

fn main() -> Result<(), Box<dyn Error>> {
    let metadata = MetadataCommand::new()
        .manifest_path(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
        .cargo_path(env::var_os("CARGO").ok_or("CARGO is absent")?)
        .other_options(vec!["--locked".into()])
        .exec()?;
    let upstream = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "tree-sitter-php" && package.source.is_some())
        .ok_or("The pinned PHP grammar is absent")?;
    let source = upstream
        .manifest_path
        .parent()
        .ok_or("No PHP source directory")?
        .as_std_path();
    let root = std::path::PathBuf::from(env::var_os("OUT_DIR").ok_or("No output directory")?)
        .join("grammar");
    fs::create_dir_all(root.join("common"))?;
    fs::copy(
        source.join("common/scanner.h"),
        root.join("common/scanner.h"),
    )?;
    let mut metadata: Value = serde_json::from_slice(&fs::read(source.join("tree-sitter.json"))?)?;
    let php = metadata["grammars"]
        .as_array_mut()
        .ok_or("No PHP grammar metadata")?
        .iter_mut()
        .find(|grammar| grammar["name"] == "php")
        .ok_or("No mixed PHP grammar metadata")?;
    php["file-types"] = json!(["php", "phtml"]);
    php["injection-regex"] = json!("^php$");
    // Keep the PHP-only grammar available without registering the file scope twice.
    let php_only = metadata["grammars"]
        .as_array_mut()
        .ok_or("No PHP grammar metadata")?
        .iter_mut()
        .find(|grammar| grammar["name"] == "php_only")
        .ok_or("No PHP-only grammar metadata")?;
    php_only["scope"] = json!("source.php_only");
    fs::write(
        root.join("tree-sitter.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("LICENSE") {
            fs::copy(entry.path(), root.join(entry.file_name()))?;
        }
    }
    fs::create_dir_all(root.join("queries"))?;
    for entry in fs::read_dir(source.join("queries"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), root.join("queries").join(entry.file_name()))?;
        }
    }
    let mut compiler = cc::Build::new();
    compiler.std("c11");
    for dialect in ["php", "php_only"] {
        let directory = root.join(dialect);
        let src = directory.join("src");
        fs::create_dir_all(&src)?;
        let mut grammar: Value =
            serde_json::from_slice(&fs::read(source.join(dialect).join("src/grammar.json"))?)?;
        correct_grammar(&mut grammar)?;
        let input = src.join("grammar.json");
        fs::write(&input, serde_json::to_vec_pretty(&grammar)?)?;
        generate_parser_in_directory(
            &directory,
            None::<&Path>,
            Some(&input),
            15,
            None,
            None,
            true,
            OptLevel::default(),
            &mut Vec::new(),
        )?;
        fs::copy(
            source.join(dialect).join("src/scanner.c"),
            src.join("scanner.c"),
        )?;
        compiler
            .include(&src)
            .file(src.join("parser.c"))
            .file(src.join("scanner.c"));
    }
    compiler.compile("entanglement-php");
    println!("cargo:root={}", root.display());
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}

/// Add the missing PHP 8.5 syntax without changing value expressions.
fn correct_grammar(grammar: &mut Value) -> Result<(), Box<dyn Error>> {
    let rules = &mut grammar["rules"];
    for pointer in [
        "/members/2/members/0/members/0/members/0",
        "/members/2/members/1/members/0",
    ] {
        let modifier = rules["use_as_clause"]
            .pointer_mut(pointer)
            .ok_or("No PHP trait adaptation modifier")?;
        if *modifier != json!({"type":"SYMBOL","name":"visibility_modifier"}) {
            return Err("The PHP trait adaptation rule has changed".into());
        }
        *modifier = json!({"type":"CHOICE","members":[
            modifier.clone(), {"type":"SYMBOL","name":"final_modifier"}
        ]});
    }
    let promotion = &mut rules["property_promotion_parameter"]["members"][1];
    if promotion["type"] != "REPEAT1" || promotion["content"]["name"] != "visibility" {
        return Err("The PHP promotion rule has changed".into());
    }
    promotion["content"] = json!({"type":"CHOICE", "members":[promotion["content"].clone(),
        {"type":"FIELD", "name":"final", "content":{"type":"SYMBOL", "name":"final_modifier"}},
        {"type":"FIELD", "name":"readonly", "content":{"type":"SYMBOL", "name":"readonly_modifier"}}]});
    let parameters = rules["property_promotion_parameter"]["members"]
        .as_array_mut()
        .ok_or("No PHP promotion members")?;
    if parameters[2]["members"][0]["name"] != "readonly" {
        return Err("The PHP readonly promotion rule has changed".into());
    }
    parameters.remove(2);
    let statement = &mut rules["expression_statement"]["members"][0];
    if *statement != json!({"type":"SYMBOL", "name":"expression"}) {
        return Err("The PHP expression statement rule has changed".into());
    }
    *statement = json!({"type":"CHOICE", "members":[statement.clone(), {"type":"SYMBOL","name":"void_expression"}]});
    rules["void_expression"] = json!({"type":"SEQ", "members":[
        {"type":"STRING","value":"("},
        {"type":"ALIAS","content":{"type":"PATTERN","value":"void","flags":"i"},"named":false,"value":"void"},
        {"type":"STRING","value":")"},
        {"type":"FIELD","name":"value","content":{"type":"SYMBOL","name":"expression"}}
    ]});
    for field in ["initialize", "condition", "update"] {
        let member = rules["for_statement"]["members"]
            .as_array_mut()
            .ok_or("No for statement members")?
            .iter_mut()
            .find(|member| member["type"] == "FIELD" && member["name"] == field)
            .ok_or("No for statement field")?;
        if member["content"]["members"][0] != json!({"type":"SYMBOL","name":"_expressions"}) {
            return Err("The PHP for statement rule has changed".into());
        }
        let value = json!({"type":"CHOICE","members":[{"type":"SYMBOL","name":"expression"},{"type":"SYMBOL","name":"void_expression"}]});
        let last = if field == "condition" {
            json!({"type":"SYMBOL","name":"expression"})
        } else {
            value.clone()
        };
        member["content"]["members"][0] = json!({"type":"SEQ","members":[
            {"type":"REPEAT","content":{"type":"SEQ","members":[value,{"type":"STRING","value":","}]}}, last
        ]});
    }
    let function_name = &mut rules["function_definition"]["members"][3]["content"];
    if *function_name != json!({"type":"SYMBOL","name":"name"}) {
        return Err("The PHP function name rule has changed".into());
    }
    *function_name = json!({"type":"CHOICE","members":[function_name.clone(),
        {"type":"ALIAS","content":{"type":"PATTERN","value":"clone","flags":"i"},"named":true,"value":"name"}
    ]});
    Ok(())
}
