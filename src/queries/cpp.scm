(function_definition (try_statement)) @metric.function
(lambda_expression) @metric.function @metric.cognitive.closure
(for_range_loop) @metric.condition @metric.cognitive.loop
(catch_clause) @metric.condition @metric.cognitive.catch
(binary_expression operator: "and" @metric.cognitive.logical_and) @metric.cognitive.logical_expression @metric.logical_condition
(binary_expression operator: "or" @metric.cognitive.logical_or) @metric.cognitive.logical_expression @metric.logical_condition
[(requires_clause) (requires_expression) (static_assert_declaration) (decltype)] @c.unevaluated
(noexcept) @c.unevaluated
((call_expression function: (identifier) @cpp.keyword) @c.unevaluated
 (#eq? @cpp.keyword "noexcept"))
