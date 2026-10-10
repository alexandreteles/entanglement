; Only raw script/style bodies are guest languages. Native TS expressions
; and frontmatter must not be reinjected. Entanglement resolves overlapping
; requests by injection.priority; higher-priority unknown labels stay pending
; rather than silently falling back to JavaScript/CSS.

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#set! injection.language "javascript")
  (#set! injection.priority "0"))

(jsx_element
  open_tag: (jsx_opening_element !attribute
    name: (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#set! injection.language "typescript")
  (#set! injection.priority "1"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    . attribute: (jsx_attribute name: (attribute_name) @_attr) .)
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "src")
  (#set! injection.language "typescript")
  (#set! injection.priority "1"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#set! injection.language "css")
  (#set! injection.priority "0"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(quoted_attribute_value (attribute_value) @injection.language)
              (unquoted_attribute_value) @injection.language]))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "type")
  (#set! injection.language @injection.language)
  (#set! injection.priority "2"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(quoted_attribute_value (attribute_value) @injection.language)
              (unquoted_attribute_value) @injection.language]))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "lang")
  (#set! injection.language @injection.language)
  (#set! injection.priority "2"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(quoted_attribute_value (attribute_value) @injection.language)
              (unquoted_attribute_value) @injection.language]))
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#eq? @_attr "lang")
  (#set! injection.language @injection.language)
  (#set! injection.priority "2"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(quoted_attribute_value (attribute_value) @injection.language)
              (unquoted_attribute_value) @injection.language]))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "type")
  (#match? @injection.language "(?i)^(module|(text|application)/(javascript|ecmascript)(;.*)?)$")
  (#set! injection.language "javascript")
  (#set! injection.priority "3"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(quoted_attribute_value (attribute_value) @injection.language)
              (unquoted_attribute_value) @injection.language]))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "type")
  (#match? @injection.language "(?i)^(application/(ld\\+)?json|importmap|speculationrules)$")
  (#set! injection.language "json")
  (#set! injection.priority "3"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "is:raw")
  (#set! injection.language "text")
  (#set! injection.priority "4"))

(jsx_element
  open_tag: (jsx_opening_element
    name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr))
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#eq? @_attr "is:raw")
  (#set! injection.language "text")
  (#set! injection.priority "4"))


; A computed language label cannot be classified statically. Keep its body
; pending, while the attribute expression itself remains server-side TS.
(jsx_element
  open_tag: (jsx_opening_element name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(jsx_expression) (template_string)]))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "type")
  (#set! injection.language "unknown")
  (#set! injection.priority "2"))

(jsx_element
  open_tag: (jsx_opening_element name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(jsx_expression) (template_string)]))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_attr "lang")
  (#set! injection.language "unknown")
  (#set! injection.priority "2"))

(jsx_element
  open_tag: (jsx_opening_element name: (tag_name) @_tag
    attribute: (jsx_attribute name: (attribute_name) @_attr
      value: [(jsx_expression) (template_string)]))
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#eq? @_attr "lang")
  (#set! injection.language "unknown")
  (#set! injection.priority "2"))
