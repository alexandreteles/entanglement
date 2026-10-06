; Svelte owns component/template structure. Guest analyzers own script bodies
; and executable expression fragments.
_ @syntax.node
(comment) @comment

; Component references and template bindings.
(element
  (start_tag (tag_name) @svelte.component.reference)
  (#match? @svelte.component.reference "^[A-Z]"))
(element
  (self_closing_tag (tag_name) @svelte.component.reference)
  (#match? @svelte.component.reference "^[A-Z]"))

(each_block binding: (pattern) @svelte.each.binding)
(each_block index: (pattern) @svelte.each.binding)
(await_block binding: (pattern) @svelte.await.binding)
(await_branch binding: (pattern) @svelte.await.binding)
(snippet_block
  parameters: (snippet_parameters
    (pattern) @svelte.snippet.parameter))
(snippet_block
  name: (snippet_name) @metric.function.name @svelte.snippet.definition) @metric.function

(const_tag expression: (expression_value) @svelte.declaration)
(declaration_tag declaration: (expression_value) @svelte.declaration)

; Template control flow.
(if_block) @metric.condition @metric.cognitive.if
(if_block expression: (_) @metric.cognitive.condition_boundary)

(else_if_clause) @metric.condition @metric.cognitive.if @metric.cognitive.else_if
(else_if_clause expression: (_) @metric.cognitive.condition_boundary)

(else_clause) @metric.cognitive.else
(each_block) @metric.condition @metric.cognitive.loop

(await_block) @metric.multiway @metric.cognitive.multiway
(await_pending) @metric.case
(await_branch) @metric.case
(await_block shorthand: (shorthand_kind) @metric.case)

; Script language selection. The adapter classifies instance vs module scripts.
(element
  (start_tag (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#set! injection.language "javascript"))
(element
  (start_tag
    (tag_name) @_tag
    (attribute (attribute_name) @_language
      (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_language "lang")
  (#match? @_value "^(ts|typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "2"))

; Styles stay outside the component execution metric context.
(element
  (start_tag (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#set! injection.language "css"))
(element
  (start_tag
    (tag_name) @_tag
    (attribute (attribute_name) @_language
      (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#eq? @_language "lang")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))

; Pattern nodes are bindings, not executable guest programs.
(expression content: (js) @injection.content
  (#set! injection.language "javascript"))
(expression content: (ts) @injection.content
  (#set! injection.language "typescript"))
(expression_value content: (js) @injection.content
  (#set! injection.language "javascript"))
(expression_value content: (ts) @injection.content
  (#set! injection.language "typescript"))
