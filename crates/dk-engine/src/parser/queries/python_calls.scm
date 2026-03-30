; Python call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct call)
;   @method_callee — the method name in a method call or attribute call
;   @call          — the entire call/decorator node (used for span)

; Direct function calls: foo()
(call
  function: (identifier) @callee) @call

; Method calls: obj.method()
(call
  function: (attribute
    attribute: (identifier) @method_callee)) @call

; Decorator direct call: @login_required
(decorator
  (identifier) @callee) @call

; Decorator method call: @app.route (without arguments)
(decorator
  (attribute
    attribute: (identifier) @method_callee)) @call

; Decorator call with arguments: @app.route("/api")
(decorator
  (call
    function: (attribute
      attribute: (identifier) @method_callee))) @call
