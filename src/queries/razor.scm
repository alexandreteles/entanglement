_ @syntax.node
[(razor_comment) (html_comment)] @comment
(razor_if) @metric.condition @metric.cognitive.if
(razor_else_if) @metric.condition @metric.cognitive.else_if
(razor_else) @metric.cognitive.else
[(razor_while) (razor_do_while) (razor_foreach)] @metric.condition @metric.cognitive.loop
(razor_for) @metric.cognitive.loop
(razor_for condition: (_)) @metric.condition
(razor_switch) @metric.multiway @metric.cognitive.multiway
(razor_switch_case) @metric.case
(razor_catch) @metric.condition @metric.cognitive.catch

([
  (razor_block) @injection.content
  (razor_implicit_expression (_) @injection.content)
  (razor_explicit_expression (_) @injection.content)
  (razor_condition (_) @injection.content)
  (razor_attribute_value (_) @injection.content)
  (razor_case_condition (_) @injection.content)
  (razor_lock (_) @injection.content)
  (razor_compound_using (_) @injection.content)
  (razor_if (_) @injection.content)
  (razor_else_if (_) @injection.content)
  (razor_else (_) @injection.content)
  (razor_while (_) @injection.content)
  (razor_do_while (_) @injection.content)
  (razor_for (_) @injection.content)
  (razor_foreach (_) @injection.content)
  (razor_try (_) @injection.content)
  (razor_catch (_) @injection.content)
  (razor_finally (_) @injection.content)
  (razor_switch_case (_) @injection.content)
  (razor_switch_default (_) @injection.content)
]
 (#set! injection.language "csharp")
 (#set! injection.include-children))
