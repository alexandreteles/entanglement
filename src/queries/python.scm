; Python functions and lambdas use one function boundary each. The declaration
; range includes its signature but decorators remain outside the function node.
(function_definition name: (identifier) @metric.function.name) @metric.function
(assignment left: (identifier) @metric.function.name right: (lambda) @metric.function)
(named_expression name: (identifier) @metric.function.name value: (lambda) @metric.function)
(lambda) @metric.function

; Cognitive complexity and decision counting.
(if_statement) @metric.cognitive.if @metric.condition
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(elif_clause) @metric.cognitive.if @metric.cognitive.else_if @metric.condition
(elif_clause condition: (_) @metric.cognitive.condition_boundary)
(if_statement alternative: (else_clause) @metric.cognitive.else)
(while_statement) @metric.cognitive.loop @metric.condition
(while_statement alternative: (else_clause) @metric.cognitive.else)
(for_statement) @metric.cognitive.loop @metric.condition
(for_statement alternative: (else_clause) @metric.cognitive.else)
(for_in_clause) @metric.cognitive.loop @metric.condition
(if_clause) @metric.cognitive.if @metric.condition
(if_clause (_) @metric.cognitive.condition_boundary)
(conditional_expression) @metric.cognitive.if @metric.condition
(except_clause) @metric.cognitive.if @metric.condition
(match_statement) @metric.cognitive.multiway @metric.multiway @metric.condition
(case_clause) @metric.case
(boolean_operator operator: "and" @metric.cognitive.logical_and) @metric.cognitive.logical_expression @metric.logical_condition
(boolean_operator operator: "or" @metric.cognitive.logical_or) @metric.cognitive.logical_expression @metric.logical_condition
(parenthesized_expression) @metric.cognitive.parentheses

(comment) @comment
_ @syntax.node

; Only a direct string literal passed to a helper named for a registered
; grammar is a guest. Prefixes, quotes, and f-string expressions stay Python.
; Repeated interpolation captures leave every host expression in Python.
(call
  function: (identifier) @injection.language
  arguments: (argument_list
    .
    (string
      (string_start) @injection.host
      (interpolation)* @injection.host
      (string_end) @injection.host) @injection.content
    .)
  (#set! injection.language @injection.language)
  (#set! injection.include-children "true")
  (#set! injection.registered-only "true"))
(call
  function: (attribute attribute: (identifier) @injection.language)
  arguments: (argument_list
    .
    (string
      (string_start) @injection.host
      (interpolation)* @injection.host
      (string_end) @injection.host) @injection.content
    .)
  (#set! injection.language @injection.language)
  (#set! injection.include-children "true")
  (#set! injection.registered-only "true"))
