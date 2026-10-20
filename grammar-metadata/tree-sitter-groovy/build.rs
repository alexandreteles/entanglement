use serde_json::{Value, json};
use std::error::Error;

#[path = "../build_support.rs"]
mod build_support;
use build_support::GrammarBuild;

fn main() -> Result<(), Box<dyn Error>> {
    let mut build = GrammarBuild::new("tree-sitter-groovy")?;
    build.parser("", correct_grammar)?;
    build.finish("entanglement-groovy");
    Ok(())
}

/// Keep a parenthesized call inside a command argument.
fn correct_grammar(grammar: &mut Value) -> Result<(), Box<dyn Error>> {
    grammar["rules"]["_juxt_argument_list"]["value"] = json!(0);
    grammar["rules"]["function_call"]["value"] = json!(3);
    Ok(())
}
