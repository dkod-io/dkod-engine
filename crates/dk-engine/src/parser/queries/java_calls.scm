; Java call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called method/function name (standalone calls)
;   @method_callee — the method name in a receiver-qualified invocation
;   @call          — the entire call node (used for span)

; Standalone method calls (no receiver): method()
; The `!object` predicate excludes method_invocations that have a receiver.
(method_invocation
  !object
  name: (identifier) @callee) @call

; Receiver method calls: obj.method() or ClassName.method()
(method_invocation
  object: (_)
  name: (identifier) @method_callee) @call

; Constructor calls: new Foo()
(object_creation_expression
  type: (type_identifier) @callee) @call
