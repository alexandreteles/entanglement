use serde_json::{Value, json};
use std::error::Error;

#[path = "../build_support.rs"]
mod build_support;
use build_support::GrammarBuild;

fn main() -> Result<(), Box<dyn Error>> {
    let mut build = GrammarBuild::new("tree-sitter-kotlin")?;
    build.parser("", correct_grammar)?;
    build.finish("entanglement-kotlin");
    Ok(())
}

/// Add context clauses and keep `context` available as a call identifier.
fn correct_grammar(grammar: &mut Value) -> Result<(), Box<dyn Error>> {
    grammar["conflicts"]
        .as_array_mut()
        .ok_or("No Kotlin conflicts")?
        .push(json!(["context_parameters", "simple_identifier"]));
    let rules = &mut grammar["rules"];
    rules["simple_identifier"]["members"]
        .as_array_mut()
        .ok_or("No Kotlin identifiers")?
        .push(json!({"type":"STRING", "value":"context"}));
    rules["context_parameters"] = json!({"type":"PREC_DYNAMIC", "value":2, "content":{
        "type":"SEQ", "members":[
            {"type":"STRING", "value":"context"}, {"type":"STRING", "value":"("},
            {"type":"CHOICE", "members":[{"type":"SEQ", "members":[
                {"type":"SYMBOL", "name":"context_parameter"},
                {"type":"REPEAT", "content":{"type":"SEQ", "members":[
                    {"type":"STRING", "value":","}, {"type":"SYMBOL", "name":"context_parameter"}]}},
                {"type":"CHOICE", "members":[{"type":"STRING", "value":","}, {"type":"BLANK"}]}
            ]}, {"type":"BLANK"}]},
            {"type":"STRING", "value":")"}
        ]
    }});
    rules["context_parameter"] = json!({"type":"CHOICE", "members":[
        {"type":"SEQ", "members":[{"type":"SYMBOL", "name":"simple_identifier"},
            {"type":"STRING", "value":":"}, {"type":"SYMBOL", "name":"_type"}]},
        {"type":"SYMBOL", "name":"_type"}
    ]});
    for (name, pointer) in [
        ("class_declaration", "/content/content/members/0/members"),
        ("class_declaration", "/content/content/members/1/members"),
        ("object_declaration", "/content/content/members"),
        ("function_declaration", "/content/members"),
        ("property_declaration", "/content/members"),
        ("secondary_constructor", "/members"),
        ("companion_object", "/members"),
    ] {
        let members = rules[name]
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .ok_or("The Kotlin declaration rule has changed")?;
        if members[0]
            != json!({"type":"CHOICE", "members":[
            {"type":"SYMBOL", "name":"modifiers"}, {"type":"BLANK"}]})
        {
            return Err("The Kotlin declaration modifiers have changed".into());
        }
        members.insert(
            0,
            json!({"type":"CHOICE", "members":[
            {"type":"SYMBOL", "name":"context_parameters"}, {"type":"BLANK"}]}),
        );
    }
    Ok(())
}
