_ @syntax.node
(comment) @comment

(method_declaration body: (_)) @metric.function
(local_function_statement body: (_)) @metric.function
(constructor_declaration body: (_)) @metric.function
(destructor_declaration body: (_)) @metric.function
(operator_declaration body: (_)) @metric.function
(conversion_operator_declaration body: (_)) @metric.function
(accessor_declaration body: (_)) @metric.function
(property_declaration value: (arrow_expression_clause)) @metric.function
(indexer_declaration "this" @metric.function.name value: (arrow_expression_clause)) @metric.function
[(lambda_expression) (anonymous_method_expression)] @metric.function
(variable_declarator name: (_) @metric.function.name [(lambda_expression) (anonymous_method_expression)] @metric.function)

(if_statement condition: (_) @metric.cognitive.condition_boundary) @metric.condition @metric.cognitive.if
(if_statement alternative: (if_statement) @metric.cognitive.else_if)
(if_statement "else" @metric.cognitive.else alternative: (_) @cs.alternative)
[(while_statement) (do_statement) (foreach_statement)] @metric.condition @metric.cognitive.loop
(for_statement) @metric.cognitive.loop
(for_statement condition: (_)) @metric.condition
(conditional_expression condition: (_) @metric.cognitive.condition_boundary) @metric.condition @metric.cognitive.if
[(switch_statement) (switch_expression)] @metric.multiway @metric.cognitive.multiway
[(switch_section) (switch_expression_arm)] @metric.case
[(when_clause) (catch_filter_clause)] @metric.condition @metric.cognitive.if
(catch_clause) @metric.condition @metric.cognitive.catch
(binary_expression operator: "&&" @metric.cognitive.logical_and) @metric.logical_condition @metric.cognitive.logical_expression
(binary_expression operator: "||" @metric.cognitive.logical_or) @metric.logical_condition @metric.cognitive.logical_expression
(and_pattern operator: "and" @metric.cognitive.logical_and) @metric.logical_condition @metric.cognitive.logical_expression
(or_pattern operator: "or" @metric.cognitive.logical_or) @metric.logical_condition @metric.cognitive.logical_expression
(binary_expression operator: "??") @metric.logical_condition
(assignment_expression operator: "??=") @metric.logical_condition
(parenthesized_expression) @metric.cognitive.parentheses
[(break_statement label: (_)) (continue_statement label: (_)) (goto_statement)] @metric.cognitive.labeled_jump
