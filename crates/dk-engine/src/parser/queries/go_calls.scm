; Go call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct or qualified)
;   @method_callee — the method name in a selector call
;   @call          — the entire call node (used for span)

; Direct function calls: foo()
(call_expression
  function: (identifier) @callee) @call

; Qualified function calls: pkg.Function()
(call_expression
  function: (selector_expression
    field: (field_identifier) @method_callee)) @call
