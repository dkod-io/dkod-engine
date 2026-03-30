; Ruby call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called method name (standalone calls)
;   @method_callee — the method name in a receiver call
;   @call          — the entire call node (used for span)

; Standalone method calls (no receiver): process(data), puts "hello"
(call
  !receiver
  method: (identifier) @callee) @call

; Receiver method calls: service.handle_request(req)
(call
  receiver: (_)
  method: (identifier) @method_callee) @call
