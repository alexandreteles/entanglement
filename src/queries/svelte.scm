; Svelte markup is analyzed like HTML. The grammar marks each template
; expression as JavaScript or TypeScript from the instance script language.
_ @syntax.node
(comment) @comment

(element
  (start_tag (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#set! injection.language "javascript"))
(element
  (start_tag
    (tag_name) @_tag
    (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @_value)))
  (raw_text) @injection.content
  (#eq? @_tag "script")
  (#eq? @_language "lang")
  (#match? @_value "^(ts|typescript)$")
  (#set! injection.language "typescript")
  (#set! injection.priority "1"))

(element
  (start_tag (tag_name) @_tag)
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#set! injection.language "css"))
(element
  (start_tag
    (tag_name) @_tag
    (attribute (attribute_name) @_language (quoted_attribute_value (attribute_value) @injection.language)))
  (raw_text) @injection.content
  (#eq? @_tag "style")
  (#eq? @_language "lang")
  (#set! injection.language @injection.language)
  (#set! injection.priority "1"))

((js) @injection.content (#set! injection.language "javascript"))
((ts) @injection.content (#set! injection.language "typescript"))
