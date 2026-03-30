; Kotlin call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct)
;   @method_callee — the method name in a navigation call
;   @call          — the entire call node (used for span)

; Direct function calls: foo()
(call_expression
  (simple_identifier) @callee) @call

; Qualified calls: obj.method()
(call_expression
  (navigation_expression
    (navigation_suffix
      (simple_identifier) @method_callee))) @call
