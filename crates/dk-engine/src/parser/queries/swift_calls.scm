; Swift call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct calls)
;   @method_callee — the method name in a navigation call (obj.method())
;   @call          — the entire call node (used for span)

; Direct function calls: process(data), print("hello")
(call_expression
  (simple_identifier) @callee) @call

; Navigation (member) calls: service.handleRequest(req)
(call_expression
  (navigation_expression
    (navigation_suffix
      (simple_identifier) @method_callee))) @call
