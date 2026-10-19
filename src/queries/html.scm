; HTML is analyzed as markup while script/style contents use language queries.
_ @syntax.node
(comment) @comment

; Default script content is JavaScript. Explicit language attributes below get
; a higher priority to select the declared grammar for the same source range.
(script_element (raw_text) @injection.content
  (#set! injection.language "javascript"))
(script_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(script_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @injection.language))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(script_element
  (start_tag (attribute (attribute_name) @_language
    [(quoted_attribute_value (attribute_value) @_value) (attribute_value) @_value]))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#match? @_value "(?i)^(ts|typescript|text/typescript|application/typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_language
    [(quoted_attribute_value (attribute_value) @_value) (attribute_value) @_value]))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#match? @_value "(?i)^(module|js|javascript|jsx|text/javascript|application/javascript|text/jsx|text/babel)$")
  (#set! injection.language "javascript")
  (#set! injection.priority "2"))
(script_element
  (start_tag (attribute (attribute_name) @_language
    [(quoted_attribute_value (attribute_value) @_value) (attribute_value) @_value]))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#match? @_value "(?i)^(tsx|text/tsx)$")
  (#set! injection.language "tsx")
  (#set! injection.priority "2"))

(style_element (raw_text) @injection.content
  (#set! injection.language "css"))
(style_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(style_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @injection.language))
  (raw_text) @injection.content
  (#match? @_language "(?i)^(lang|type)$")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))
(style_element
  (start_tag (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#match? @_language "(?i)^lang$")
  (#match? @_value "(?i)^(css|text/css)$")
  (#set! injection.language "css")
  (#set! injection.priority "2"))
(style_element
  (start_tag (attribute (attribute_name) @_language (attribute_value) @_value))
  (raw_text) @injection.content
  (#match? @_language "(?i)^lang$")
  (#match? @_value "(?i)^(css|text/css)$")
  (#set! injection.language "css")
  (#set! injection.priority "2"))
(style_element
  (start_tag (attribute (attribute_name) @_type (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#match? @_type "(?i)^type$")
  (#match? @_value "(?i)^(css|text/css)$")
  (#set! injection.language "css")
  (#set! injection.priority "2"))
(style_element
  (start_tag (attribute (attribute_name) @_type (attribute_value) @_value))
  (raw_text) @injection.content
  (#match? @_type "(?i)^type$")
  (#match? @_value "(?i)^(css|text/css)$")
  (#set! injection.language "css")
  (#set! injection.priority "2"))
