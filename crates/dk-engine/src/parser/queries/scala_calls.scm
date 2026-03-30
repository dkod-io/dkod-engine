; Scala call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct)
;   @method_callee — the method name in a qualified/field call
;   @call          — the entire call node (used for span)

; Direct function calls: foo()
(call_expression
  function: (identifier) @callee) @call

; Qualified calls: obj.method()
(call_expression
  function: (field_expression
    field: (identifier) @method_callee)) @call
