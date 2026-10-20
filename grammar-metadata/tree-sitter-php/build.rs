use serde_json::{Value, json};
use std::{error::Error, fs};

#[path = "../build_support.rs"]
mod build_support;
use build_support::GrammarBuild;

fn main() -> Result<(), Box<dyn Error>> {
    let mut build = GrammarBuild::new("tree-sitter-php")?;
    fs::create_dir_all(build.root.join("common"))?;
    fs::copy(
        build.source.join("common/scanner.h"),
        build.root.join("common/scanner.h"),
    )?;
    let mut metadata: Value =
        serde_json::from_slice(&fs::read(build.root.join("tree-sitter.json"))?)?;
    let grammars = metadata["grammars"]
        .as_array_mut()
        .ok_or("No PHP grammar metadata")?;
    let php = grammars
        .iter_mut()
        .find(|grammar| grammar["name"] == "php")
        .ok_or("No mixed PHP grammar metadata")?;
    php["file-types"] = json!(["php", "phtml"]);
    php["injection-regex"] = json!("^php$");
    // Keep the PHP-only grammar available without registering the file scope twice.
    let php_only = grammars
        .iter_mut()
        .find(|grammar| grammar["name"] == "php_only")
        .ok_or("No PHP-only grammar metadata")?;
    php_only["scope"] = json!("source.php_only");
    fs::write(
        build.root.join("tree-sitter.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    for dialect in ["php", "php_only"] {
        build.parser(dialect, correct_grammar)?;
    }
    build.finish("entanglement-php");
    Ok(())
}

/// Apply the PHP syntax corrections without changing value expressions.
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
            modifier.take(), {"type":"SYMBOL","name":"final_modifier"}
        ]});
    }
    let promotion = &mut rules["property_promotion_parameter"]["members"][1];
    if promotion["type"] != "REPEAT1" || promotion["content"]["name"] != "visibility" {
        return Err("The PHP promotion rule has changed".into());
    }
    promotion["content"] = json!({"type":"CHOICE", "members":[promotion["content"].take(),
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
    *statement = json!({"type":"CHOICE", "members":[statement.take(), {"type":"SYMBOL","name":"void_expression"}]});
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
    *function_name = json!({"type":"CHOICE","members":[function_name.take(),
        {"type":"ALIAS","content":{"type":"PATTERN","value":"clone","flags":"i"},"named":true,"value":"name"}
    ]});
    Ok(())
}
