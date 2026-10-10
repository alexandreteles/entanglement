; Supplement Entanglement's existing web.scm + typescript.scm.
; Frontmatter and expressions are already native TS syntax: NEVER inject them
; again or capture component tags as synthetic branches/loops.
(html_comment) @comment

(frontmatter) @astro.frontmatter
[(jsx_text) (cdata)] @astro.text

(jsx_opening_element name: (tag_name) @astro.component.reference
  (#match? @astro.component.reference "^[A-Z]|\\.")
  (#not-eq? @astro.component.reference "Fragment"))
(jsx_self_closing_element name: (tag_name) @astro.component.reference
  (#match? @astro.component.reference "^[A-Z]|\\.")
  (#not-eq? @astro.component.reference "Fragment"))

; The adapter decides script scope, exports, and define:vars serialization.
(jsx_element
  open_tag: (jsx_opening_element name: (tag_name) @_script)
  (raw_text) @astro.script
  (#eq? @_script "script"))
(jsx_element
  open_tag: (jsx_opening_element name: (tag_name) @_style)
  (raw_text) @astro.style
  (#eq? @_style "style"))
(jsx_attribute name: (attribute_name) @_directive value: (jsx_expression) @astro.define_vars
  (#eq? @_directive "define:vars"))
