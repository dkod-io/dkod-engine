; Ruby symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Ruby visibility: all symbols are public by default. Ruby uses
; method-level `private`/`protected` calls but those don't appear as
; modifiers on the method AST node, so we default everything to Public.
;
; IMPORTANT: class and module names use `(constant)`, NOT `(identifier)`.

; ── Classes ──
(class
  name: (constant) @name) @definition.class

; ── Modules ──
(module
  name: (constant) @name) @definition.module

; ── Instance methods ──
(method
  name: (identifier) @name) @definition.function

; ── Singleton (class) methods: def self.method_name ──
(singleton_method
  name: (identifier) @name) @definition.function
