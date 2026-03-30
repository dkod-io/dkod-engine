; Rust call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function/macro name (direct or scoped)
;   @method_callee — the method name in a method-call expression
;   @call          — the entire call node (used for span)

; Direct function calls: foo()
(call_expression
  function: (identifier) @callee) @call

; Scoped function calls: Path::new(), std::fs::read()
(call_expression
  function: (scoped_identifier) @callee) @call

; Method calls: user.save()
(call_expression
  function: (field_expression
    field: (field_identifier) @method_callee)) @call

; Macro invocations: println!(), todo!()
(macro_invocation
  macro: (identifier) @callee) @call
