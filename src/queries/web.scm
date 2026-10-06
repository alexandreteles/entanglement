; Shared JavaScript, TypeScript, and TSX syntax facts. JSX stays JavaScript
; syntax so its embedded expressions retain their host language semantics.
_ @syntax.node

(comment) @comment

; Function boundaries cover declarations, expressions, generators, arrows,
; and class/object methods. Bound function expressions get a name where known.
(function_declaration name: (_) @metric.function.name @definition.name) @metric.function @definition.function
(generator_function_declaration name: (_) @metric.function.name @definition.name) @metric.function @definition.function
(function_expression) @metric.function
(function_expression name: (_) @metric.function.name @definition.name)
  @metric.function @definition.function
(generator_function) @metric.function
(method_definition name: (_) @metric.function.name @definition.name) @metric.function @definition.method

(variable_declarator name: (identifier) @metric.function.name value: (arrow_function) @metric.function)
(variable_declarator name: (identifier) @metric.function.name value: (function_expression) @metric.function)
(variable_declarator name: (identifier) @metric.function.name value: (generator_function) @metric.function)
(pair key: (_) @metric.function.name value: (arrow_function) @metric.function)
(pair key: (_) @metric.function.name value: (function_expression) @metric.function)
(assignment_expression left: (identifier) @metric.function.name right: (arrow_function) @metric.function)
(assignment_expression left: (identifier) @metric.function.name right: (function_expression) @metric.function)
(arrow_function) @metric.function

(variable_declarator name: (identifier) @definition.name) @definition.variable

(class_declaration name: (_) @definition.name) @definition.class

; Branches and loops contribute to both complexity measures.
(if_statement) @metric.cognitive.if @metric.condition
(if_statement condition: (_) @metric.cognitive.condition_boundary)
(else_clause (_) @metric.cognitive.else
  (#not-match? @metric.cognitive.else "^\\s*if\\b"))
(if_statement alternative: (else_clause (if_statement) @metric.cognitive.else_if))
(for_statement) @metric.cognitive.loop @metric.condition
(for_in_statement) @metric.cognitive.loop @metric.condition
(while_statement) @metric.cognitive.loop @metric.condition
(do_statement) @metric.cognitive.loop @metric.condition
(switch_statement) @metric.cognitive.multiway @metric.multiway @metric.condition
(switch_case) @metric.case
(ternary_expression) @metric.cognitive.if @metric.condition
(catch_clause) @metric.cognitive.if @metric.condition

; Logical operators are grouped by operator changes for cognitive complexity.
(binary_expression operator: "&&" @metric.cognitive.logical_and) @metric.cognitive.logical_expression
(binary_expression operator: "||" @metric.cognitive.logical_or) @metric.cognitive.logical_expression
(binary_expression operator: ["&&" "||" "??"] @metric.logical_condition)
(parenthesized_expression) @metric.cognitive.parentheses

; Runtime names and call targets feed the language-neutral resolver.
(identifier) @reference.value
(call_expression function: (identifier) @reference.call)
(new_expression constructor: (identifier) @reference.call)
(call_expression function: (member_expression property: (property_identifier) @reference.method))
(member_expression object: (identifier) @reference.namespace.object property: (_) @reference.namespace.property) @reference.namespace

; Imports, exports, and local binding patterns are normalized by the adapter.
(import_statement) @import
(export_statement) @export
(variable_declarator name: (_) @local.binding)
(formal_parameters (_) @local.parameter)
(arrow_function parameter: (_) @local.parameter)
(catch_clause parameter: (_) @local.catch)
(for_in_statement left: (_) @local.loop)

; Tagged template labels name their embedded grammar. Exclude each host
; substitution from the guest ranges while retaining the expression in JS/TS.
(call_expression
  function: (identifier) @injection.language
  arguments: (template_string
    "`" @injection.host
    (template_substitution)* @injection.host
    "`" @injection.host) @injection.content
  (#set! injection.language @injection.language)
  (#set! injection.include-children "true"))
(call_expression
  function: (member_expression property: (property_identifier) @injection.language)
  arguments: (template_string
    "`" @injection.host
    (template_substitution)* @injection.host
    "`" @injection.host) @injection.content
  (#set! injection.language @injection.language)
  (#set! injection.include-children "true"))
