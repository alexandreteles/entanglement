; C++ adds reference declarators, lambdas, range loops, handlers, and
; alternative spellings of the logical operators to the C rules.
(function_definition
  declarator: (reference_declarator (function_declarator declarator: (_) @metric.function.name))) @metric.function
(init_declarator
  declarator: (identifier) @metric.function.name
  value: (lambda_expression) @metric.function)
(lambda_expression) @metric.function

(for_range_loop) @metric.cognitive.loop @metric.condition
(catch_clause) @metric.cognitive.catch @metric.condition
(binary_expression operator: "and" @metric.cognitive.logical_and) @metric.cognitive.logical_expression @metric.logical_condition
(binary_expression operator: "or" @metric.cognitive.logical_or) @metric.cognitive.logical_expression @metric.logical_condition
