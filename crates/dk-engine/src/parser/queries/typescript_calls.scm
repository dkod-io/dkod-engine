; TypeScript/JavaScript call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct call)
;   @method_callee — the method name in a method call
;   @call          — the entire call node (used for span)

; Direct function calls: foo()
(call_expression
  function: (identifier) @callee) @call

; Method calls: obj.method()
(call_expression
  function: (member_expression
    property: (property_identifier) @method_callee)) @call

; Constructor calls: new Foo()
(new_expression
  constructor: (identifier) @callee) @call
