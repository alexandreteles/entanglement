; Kotlin callable syntax and executable initialization scopes.
(function_declaration) @metric.function
(anonymous_function) @metric.function
(lambda_literal) @metric.function
(primary_constructor) @metric.function
(secondary_constructor) @metric.function
(anonymous_initializer) @metric.function
(property_declaration) @metric.function
(getter) @metric.function
(setter) @metric.function
(destructuring_declaration) @metric.function
(enum_entry) @metric.function

; Cyclomatic decisions. `when` contributes one point per arm after the first;
; guards, loop forms, catches, Elvis operators, and short-circuit operators
; contribute one each.
(if_expression condition: (_) @metric.condition) @metric.cognitive.if
(if_expression alternative: (control_structure_body) @metric.cognitive.else)
(if_expression alternative: (control_structure_body (if_expression) @metric.cognitive.else_if))
(for_statement) @metric.condition @metric.cognitive.loop
(while_statement) @metric.condition @metric.cognitive.loop
(do_while_statement) @metric.condition @metric.cognitive.loop
(when_expression) @metric.multiway @metric.cognitive.multiway
(when_entry) @metric.case
(guard_condition) @metric.condition @metric.cognitive.if
(catch_block) @metric.condition @metric.cognitive.catch
(jump_expression "break@" (label)) @metric.cognitive.labeled_jump
(jump_expression "continue@" (label)) @metric.cognitive.labeled_jump
(elvis_expression "?:" @metric.condition)
(conjunction_expression "&&" @metric.logical_condition @metric.cognitive.logical_and) @metric.cognitive.logical_expression
(disjunction_expression "||" @metric.logical_condition @metric.cognitive.logical_or) @metric.cognitive.logical_expression
(parenthesized_expression) @metric.cognitive.parentheses

(line_comment) @comment
(multiline_comment) @comment
_ @syntax.node
