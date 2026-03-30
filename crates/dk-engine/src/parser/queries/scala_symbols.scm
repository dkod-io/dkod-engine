; Scala symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Scala visibility is handled in resolve_visibility() by checking the
; `access_modifier` child. Default is Public.

; ── Classes ──
(class_definition
  name: (identifier) @name) @definition.class

; ── Objects (companion objects / singletons) ──
(object_definition
  name: (identifier) @name) @definition.module

; ── Traits ──
(trait_definition
  name: (identifier) @name) @definition.trait

; ── Enums (Scala 3) ──
(enum_definition
  name: (identifier) @name) @definition.enum

; ── Functions (with body) ──
(function_definition
  name: (identifier) @name) @definition.function

; ── Function declarations (abstract, no body) ──
(function_declaration
  name: (identifier) @name) @definition.function

; ── Val definitions ──
(val_definition
  pattern: (identifier) @name) @definition.const

; ── Var definitions ──
(var_definition
  pattern: (identifier) @name) @definition.variable
