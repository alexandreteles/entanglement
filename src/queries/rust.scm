; Keep language rules here. Metrics use only the captures in this file.

_ @syntax.node

(line_comment) @comment
(block_comment) @comment

(function_item) @metric.function

(if_expression) @metric.condition
(while_expression) @metric.condition
(for_expression) @metric.condition
(let_declaration alternative: (block)) @metric.condition
(match_pattern condition: (_)) @metric.condition
(let_chain "&&" @metric.logical_condition)
(try_expression) @metric.condition
(binary_expression operator: ["&&" "||"]) @metric.logical_condition
(match_expression) @metric.multiway
(match_arm) @metric.case

(const_item name: (identifier) @name) @definition.constant
(static_item name: (identifier) @name) @definition.static

(use_declaration argument: (_) @import.path) @import
(scoped_identifier) @reference.path
(scoped_type_identifier) @reference.path
(identifier) @reference.value
(type_identifier) @reference.type
(call_expression function: (generic_function function: (identifier) @name)) @reference.call
(call_expression function: (generic_function function: (scoped_identifier) @reference.path))
(call_expression function: (generic_function function: (field_expression field: (field_identifier) @name))) @reference.call

(let_declaration pattern: (_) @local.pattern) @local.declaration
(parameter pattern: (_) @local.pattern) @local.parameter
(self_parameter) @local.parameter
(for_expression pattern: (_) @local.pattern) @local.loop
(let_condition pattern: (_) @local.pattern) @local.condition
(match_arm pattern: (match_pattern) @local.pattern) @local.match
(closure_expression parameters: (closure_parameters (_) @local.pattern)) @local.closure

((macro_invocation
   macro: (identifier) @_rshtml_macro
   (token_tree) @injection.content)
 (#eq? @_rshtml_macro "v")
 (#set! injection.language "html")
 (#set! injection.priority "1")
 (#set! injection.include-children))
