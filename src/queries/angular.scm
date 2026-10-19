; Angular expressions stay in the Angular tree.
(arrow_function) @metric.function
(if_statement) @metric.condition @metric.cognitive.if
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(else_if_statement) @metric.condition @metric.cognitive.if @metric.cognitive.else_if
(else_if_statement condition: (_) @metric.cognitive.condition_boundary)
(else_statement) @metric.cognitive.else
(for_statement) @metric.condition @metric.cognitive.loop
(switch_statement) @metric.condition @metric.multiway @metric.cognitive.multiway
(case_statement) @metric.case
(default_statement) @metric.case
(ternary_expression) @metric.condition @metric.cognitive.if
(conditional_expression) @metric.logical_condition @metric.cognitive.logical_expression
(conditional_expression
  (conditional_operator) @metric.cognitive.logical_and
  (#eq? @metric.cognitive.logical_and "&&"))
(conditional_expression
  (conditional_operator) @metric.cognitive.logical_or
  (#eq? @metric.cognitive.logical_or "||"))
(binary_expression operator: "&&" @metric.cognitive.logical_and)
  @metric.logical_condition @metric.cognitive.logical_expression
(binary_expression operator: "||" @metric.cognitive.logical_or)
  @metric.logical_condition @metric.cognitive.logical_expression
(nullish_coalescing_expression) @metric.logical_condition
(group) @metric.cognitive.parentheses
(structural_directive
  (identifier) @_directive
  (#match? @_directive "^(ngIf|ngFor|ngSwitchCase)$"))
  @metric.condition @metric.cognitive.condition_boundary
(element
  (start_tag (attribute (structural_directive (identifier) @_directive)))
  (#eq? @_directive "ngFor")) @metric.cognitive.loop
(element
  (start_tag (attribute (structural_directive (identifier) @_directive)))
  (#match? @_directive "^(ngIf|ngSwitchCase)$")) @metric.cognitive.if
