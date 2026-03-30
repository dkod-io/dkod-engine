; C++ call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct or qualified)
;   @method_callee — the method name in a member call (obj.method() or obj->method())
;   @call          — the entire call node (used for span)

; Direct function calls: foo()
(call_expression
  function: (identifier) @callee) @call

; Qualified function calls: std::sort(), MyClass::method()
(call_expression
  function: (qualified_identifier) @callee) @call

; Member function calls: obj.method()
(call_expression
  function: (field_expression
    field: (field_identifier) @method_callee)) @call
