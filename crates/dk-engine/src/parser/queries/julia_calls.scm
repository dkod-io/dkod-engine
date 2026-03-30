; Julia call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct)
;   @method_callee — the method name in a field call
;   @call          — the entire call node (used for span)

; Direct function calls: foo(x)
(call_expression
  (identifier) @callee) @call

; Qualified calls: Mod.func(x)
(call_expression
  (field_expression
    (identifier) @method_callee .)) @call
