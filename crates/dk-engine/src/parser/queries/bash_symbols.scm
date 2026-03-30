; Bash symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Bash visibility: all symbols are public (no visibility concept).
; NOTE: Bash function names use `(word)`, NOT `(identifier)`.
; Only functions are meaningful symbols in Bash.

; ── Functions: function foo() { ... } or foo() { ... } ──
(function_definition
  name: (word) @name) @definition.function
