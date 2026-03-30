; Python symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Python visibility is name-based (_foo = private), handled in resolve_visibility().
; Decorated definitions match the inner function/class; adjust_symbol() expands
; the span to include the decorator.

; ── Functions ──
(function_definition
  name: (identifier) @name) @definition.function

; ── Classes ──
(class_definition
  name: (identifier) @name) @definition.class

; ── Module-level variable assignments ──
; e.g. MAX_RETRIES = 3
; Anchored to `module` root to avoid capturing local variables inside
; function/class bodies.
(module
  (expression_statement
    (assignment
      left: (identifier) @name)) @definition.variable)
