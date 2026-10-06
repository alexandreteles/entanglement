; Go functions, methods, and function literals use one function boundary each.
; A literal bound to one name in a declaration or assignment takes that name.
(function_declaration) @metric.function
(method_declaration) @metric.function
(short_var_declaration
  left: (expression_list . (identifier) @metric.function.name .)
  right: (expression_list . (func_literal) @metric.function .))
(assignment_statement
  left: (expression_list . (identifier) @metric.function.name .)
  right: (expression_list . (func_literal) @metric.function .))
(var_spec
  . name: (identifier) @metric.function.name .
  value: (expression_list . (func_literal) @metric.function .))
(func_literal) @metric.function

; Cognitive complexity and decision counting. A `for` without a clause or
; condition repeats unconditionally, so only its loop nesting counts.
(if_statement) @metric.cognitive.if @metric.condition
(if_statement initializer: (_) @metric.cognitive.condition_boundary)
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(if_statement alternative: (block) @metric.cognitive.else)
(if_statement alternative: (if_statement) @metric.cognitive.else_if)
(for_statement) @metric.cognitive.loop
(for_statement . (_) (block)) @metric.condition
(expression_switch_statement) @metric.cognitive.multiway @metric.multiway @metric.condition
(type_switch_statement) @metric.cognitive.multiway @metric.multiway @metric.condition
(select_statement) @metric.cognitive.multiway @metric.multiway @metric.condition
(expression_case) @metric.case
(type_case) @metric.case
(communication_case) @metric.case
(binary_expression operator: "&&" @metric.cognitive.logical_and) @metric.cognitive.logical_expression @metric.logical_condition
(binary_expression operator: "||" @metric.cognitive.logical_or) @metric.cognitive.logical_expression @metric.logical_condition
(parenthesized_expression) @metric.cognitive.parentheses
(break_statement (label_name)) @metric.cognitive.labeled_jump
(continue_statement (label_name)) @metric.cognitive.labeled_jump
(goto_statement) @metric.cognitive.labeled_jump

(comment) @comment
_ @syntax.node

; Declarations inside a function body become local bindings in the adapter.
(package_clause (package_identifier) @definition.package)
(function_declaration name: (identifier) @definition.name) @definition.function
(method_declaration name: (field_identifier) @definition.name) @definition.method
(type_spec name: (type_identifier) @definition.name) @definition.type
(type_alias name: (type_identifier) @definition.name) @definition.alias
(const_spec name: (identifier) @definition.name) @definition.constant
(var_spec name: (identifier) @definition.name) @definition.variable
(import_spec) @import

(parameter_declaration name: (identifier) @local.parameter)
(variadic_parameter_declaration name: (identifier) @local.parameter)
(type_parameter_declaration name: (identifier) @local.parameter)
(short_var_declaration left: (expression_list (identifier) @local.declaration))
(range_clause left: (expression_list (identifier) @local.declaration) ":=")
(receive_statement left: (expression_list (identifier) @local.declaration) ":=")
(type_switch_statement alias: (expression_list (identifier) @local.declaration))

; A composite literal key names a struct field or evaluates a map key; only
; the literal's type can decide, so the key is not reported.
(keyed_element . (literal_element (identifier) @reference.ignored))

(call_expression function: (identifier) @reference.call)
(call_expression
  function: (selector_expression operand: (identifier) field: (field_identifier)) @reference.call)
(selector_expression operand: (identifier) field: (field_identifier)) @reference.qualified
(qualified_type package: (package_identifier) name: (type_identifier)) @reference.qualified_type
(type_identifier) @reference.type
((identifier) @reference.value (#not-eq? @reference.value "_"))
