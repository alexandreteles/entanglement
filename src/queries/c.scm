_ @syntax.node
(comment) @comment
(function_definition body: (_)) @metric.function
(if_statement) @metric.condition @metric.cognitive.if
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(else_clause) @metric.cognitive.else
(else_clause (if_statement) @metric.cognitive.else_if)
(while_statement) @metric.condition @metric.cognitive.loop
(do_statement) @metric.condition @metric.cognitive.loop
(for_statement) @metric.cognitive.loop
(for_statement condition: (_)) @metric.condition
(switch_statement) @metric.cognitive.multiway
(case_statement value: (_)) @metric.condition
(conditional_expression) @metric.condition @metric.cognitive.if
(conditional_expression condition: (_) @metric.cognitive.condition_boundary)
(binary_expression operator: "&&" @metric.cognitive.logical_and) @metric.cognitive.logical_expression @metric.logical_condition
(binary_expression operator: "||" @metric.cognitive.logical_or) @metric.cognitive.logical_expression @metric.logical_condition
(parenthesized_expression) @metric.cognitive.parentheses
(goto_statement) @metric.cognitive.labeled_jump
(preproc_if condition: (_) @c.unevaluated)
(preproc_elif condition: (_) @c.unevaluated)
(primitive_type) @c.keyword
