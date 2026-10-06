; Type-only definitions and references layered on the shared JS/TS query.
(interface_declaration name: (type_identifier) @definition.name) @definition.interface
(type_alias_declaration name: (type_identifier) @definition.name) @definition.type
(enum_declaration name: (identifier) @definition.name) @definition.enum
(public_field_definition
  name: (_) @metric.function.name
  value: (arrow_function) @metric.function @definition.function)
(type_identifier) @reference.type

; TypeScript parameters have wrapper nodes whose pattern field carries the
; binding pattern; the adapter ignores annotations and default-value expressions.
(required_parameter pattern: (_) @local.parameter)
(optional_parameter pattern: (_) @local.parameter)
