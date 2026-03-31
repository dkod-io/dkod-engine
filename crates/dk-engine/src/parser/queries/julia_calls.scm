; Julia call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee        — the called function name (direct)
;   @method_callee — the method name in a field call
;   @call          — the entire call node (used for span)
;
; KNOWN LIMITATION: tree-sitter-julia represents function/macro definition
; signatures as `call_expression` inside a `signature` node — the same
; structure as actual function calls. Tree-sitter queries cannot exclude
; nodes based on parent context (`#not-has-parent?` does not exist).
; As a result, `function factorial(n)` will produce a false call to
; `factorial`. The engine's call-graph resolver tolerates extra edges
; because they are pruned during cross-file resolution: when building
; the final call graph, a callee name is resolved against the symbol
; tables of *other* files. For exported functions the false self-edge
; is harmless (it merges with the real cross-file edge). For private/
; unexported functions whose name never appears in another file's symbol
; table, the unresolved edge is silently dropped. A future improvement
; could post-filter calls whose span overlaps with a symbol definition
; in the same file.

; Direct function calls: foo(x)
(call_expression
  (identifier) @callee) @call

; Qualified calls: Mod.func(x)
(call_expression
  (field_expression
    (identifier) @method_callee .)) @call
