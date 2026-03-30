; PHP call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (standalone calls)
;   @method_callee — the method name in a member call ($obj->method())
;   @call          — the entire call node (used for span)

; Standalone function calls: validate($req)
(function_call_expression
  function: (name) @callee) @call

; Member method calls: $service->processRequest($req)
(member_call_expression
  name: (name) @method_callee) @call

; Constructor calls: new UserService("admin")
(object_creation_expression
  (name) @callee) @call
