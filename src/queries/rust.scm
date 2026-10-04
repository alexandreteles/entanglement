; Keep language rules here. Metrics use only the captures in this file.

_ @syntax.node

(line_comment) @comment
(block_comment) @comment

(function_item) @metric.function

(if_expression) @metric.cognitive.if
(if_expression condition: (_) @metric.cognitive.condition_boundary)
(else_clause (block)) @metric.cognitive.else
(if_expression alternative: (else_clause (if_expression) @metric.cognitive.else_if))
(while_expression) @metric.cognitive.loop
(for_expression) @metric.cognitive.loop
(loop_expression) @metric.cognitive.loop
(let_declaration alternative: (block)) @metric.cognitive.let_else
(let_declaration value: (_) @metric.cognitive.condition_boundary alternative: (block))
(match_expression) @metric.cognitive.multiway
(match_pattern condition: (_) @metric.cognitive.if)
(let_chain "&&" @metric.cognitive.logical_and) @metric.cognitive.logical_expression
(binary_expression operator: "&&" @metric.cognitive.logical_and) @metric.cognitive.logical_expression
(binary_expression operator: "||" @metric.cognitive.logical_or) @metric.cognitive.logical_expression
(parenthesized_expression) @metric.cognitive.parentheses
(closure_expression) @metric.cognitive.closure
(break_expression (label)) @metric.cognitive.labeled_jump
(continue_expression (label)) @metric.cognitive.labeled_jump

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
