; C# call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function/method name (direct calls)
;   @method_callee — the method name in a member access call
;   @call          — the entire call node (used for span)

; Direct function/method calls: Validate(req)
(invocation_expression
  function: (identifier) @callee) @call

; Member access calls: service.ProcessRequest(req), Console.WriteLine("done")
(invocation_expression
  function: (member_access_expression
    name: (identifier) @method_callee)) @call

; Constructor calls: new UserService("admin")
(object_creation_expression
  type: (identifier) @callee) @call
