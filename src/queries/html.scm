; HTML is analyzed as markup while script/style contents use language queries.
_ @syntax.node
(comment) @comment

; Default script content is JavaScript. Explicit language attributes below get
; a higher priority so TypeScript is selected for the same source range.
(script_element (raw_text) @injection.content
  (#set! injection.language "javascript"))
(script_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content
  (#match? @_language "^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(script_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @injection.language))
  (raw_text) @injection.content
  (#match? @_language "^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(script_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_language "lang")
  (#match? @_value "^(ts|typescript|text/typescript|application/typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @_value))
  (raw_text) @injection.content
  (#eq? @_language "lang")
  (#match? @_value "^(ts|typescript|text/typescript|application/typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_type (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_type "type")
  (#match? @_value "^(text/typescript|application/typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_type (attribute_value) @_value))
  (raw_text) @injection.content
  (#eq? @_type "type")
  (#match? @_value "^(text/typescript|application/typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_language "lang")
  (#match? @_value "^(js|javascript|jsx|text/javascript|application/javascript)$")
  (#set! injection.language "javascript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @_value))
  (raw_text) @injection.content
  (#eq? @_language "lang")
  (#match? @_value "^(js|javascript|jsx|text/javascript|application/javascript)$")
  (#set! injection.language "javascript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_type (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_type "type")
  (#match? @_value "^(module|text/javascript|application/javascript)$")
  (#set! injection.language "javascript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_type (attribute_value) @_value))
  (raw_text) @injection.content
  (#eq? @_type "type")
  (#match? @_value "^(module|text/javascript|application/javascript)$")
  (#set! injection.language "javascript")
  (#set! injection.priority "2"))

(style_element (raw_text) @injection.content
  (#set! injection.language "css"))
(style_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content
  (#match? @_language "^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(style_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @injection.language))
  (raw_text) @injection.content
  (#match? @_language "^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(style_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_language "lang")
  (#match? @_value "^(css|text/css)$")
  (#set! injection.language "css")
  (#set! injection.priority "2"))
(style_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @_value))
  (raw_text) @injection.content
  (#eq? @_language "lang")
  (#match? @_value "^(css|text/css)$")
  (#set! injection.language "css")
  (#set! injection.priority "2"))
