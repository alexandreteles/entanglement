; Each function body owns its decisions. The name sits at the end of a
; declarator chain, which may wrap the function declarator in pointers.
(function_definition
  declarator: (function_declarator declarator: (_) @metric.function.name)) @metric.function
(function_definition
  declarator: (_ declarator: (function_declarator declarator: (_) @metric.function.name))) @metric.function
(function_definition
  declarator: (_ declarator: (_ declarator: (function_declarator declarator: (_) @metric.function.name)))) @metric.function
(function_definition) @metric.function

; A `for` without a condition repeats unconditionally, so only its loop nesting counts.
(if_statement) @metric.cognitive.if @metric.condition
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(else_clause (statement) @metric.cognitive.else
  (#not-match? @metric.cognitive.else "^if\\b"))
(if_statement alternative: (else_clause (if_statement) @metric.cognitive.else_if))
(for_statement) @metric.cognitive.loop
(for_statement condition: (_)) @metric.condition
(while_statement) @metric.cognitive.loop @metric.condition
(do_statement) @metric.cognitive.loop @metric.condition
(switch_statement) @metric.cognitive.multiway
(case_statement value: (_)) @metric.condition
(conditional_expression) @metric.cognitive.if @metric.condition
(conditional_expression condition: (_) @metric.cognitive.condition_boundary)
(binary_expression operator: "&&" @metric.cognitive.logical_and) @metric.cognitive.logical_expression @metric.logical_condition
(binary_expression operator: "||" @metric.cognitive.logical_or) @metric.cognitive.logical_expression @metric.logical_condition
(parenthesized_expression) @metric.cognitive.parentheses
(goto_statement) @metric.cognitive.labeled_jump

(comment) @comment
_ @syntax.node
