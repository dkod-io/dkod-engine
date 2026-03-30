; Java call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called method/function name
;   @method_callee — the method name in a chained method invocation
;   @call          — the entire call node (used for span)

; Method invocations: obj.method() or method()
; method_invocation has a `name` field for the method identifier and
; an optional `object` field for the receiver.

; Standalone method calls: method() or ClassName.method()
(method_invocation
  name: (identifier) @callee) @call

; Constructor calls: new Foo()
(object_creation_expression
  type: (type_identifier) @callee) @call
