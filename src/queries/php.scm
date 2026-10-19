; Named and anonymous callable bodies each own their decisions.
(function_definition) @metric.function
(method_declaration body: (_)) @metric.function
(anonymous_function) @metric.function
(arrow_function) @metric.function
(property_hook body: (_)) @metric.function

(assignment_expression
  left: (variable_name) @metric.function.name
  right: [(anonymous_function) (arrow_function)] @metric.function)
(property_hook (name) @metric.function.name body: (_)) @metric.function

(if_statement) @metric.condition @metric.cognitive.if
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(else_if_clause) @metric.condition @metric.cognitive.else_if @metric.cognitive.if
(else_if_clause condition: (_) @metric.cognitive.condition_boundary)
(else_clause) @metric.cognitive.else
[(while_statement) (do_statement) (foreach_statement)] @metric.condition @metric.cognitive.loop
(for_statement) @metric.cognitive.loop
(for_statement condition: (_)) @metric.condition
(conditional_expression) @metric.condition @metric.cognitive.if
(conditional_expression condition: (_) @metric.cognitive.condition_boundary)
(switch_statement) @metric.cognitive.multiway
(case_statement) @metric.condition
(match_expression) @metric.cognitive.multiway
(match_conditional_expression) @metric.condition
(catch_clause) @metric.condition @metric.cognitive.catch

(binary_expression operator: ["&&" "and"] @metric.cognitive.logical_and) @metric.logical_condition @metric.cognitive.logical_expression
(binary_expression operator: ["||" "or"] @metric.cognitive.logical_or) @metric.logical_condition @metric.cognitive.logical_expression
(binary_expression operator: ["xor" "??"]) @metric.logical_condition
(parenthesized_expression) @metric.cognitive.parentheses
(goto_statement) @metric.cognitive.labeled_jump
(break_statement (integer)) @metric.cognitive.labeled_jump
(continue_statement (integer)) @metric.cognitive.labeled_jump

(comment) @comment
_ @syntax.node

; Markup uses the existing HTML analyzer and its script/style injections.
((text) @injection.content
 (#set! injection.language "html")
 (#set! injection.include-children "true"))
