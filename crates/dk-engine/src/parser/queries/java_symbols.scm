; Java symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;   @modifiers         — optional modifiers (e.g. "public static final")
;
; Java visibility: check @modifiers for public/private/protected keywords.
; Default (no modifier) is package-private, mapped to Private.
;
; The `(modifiers)?` syntax produces a single match per node regardless
; of whether modifiers are present.

; ── Classes ──
(class_declaration
  (modifiers)? @modifiers
  name: (identifier) @name) @definition.class

; ── Interfaces ──
(interface_declaration
  (modifiers)? @modifiers
  name: (identifier) @name) @definition.interface

; ── Enums ──
(enum_declaration
  (modifiers)? @modifiers
  name: (identifier) @name) @definition.enum

; ── Methods ──
(method_declaration
  (modifiers)? @modifiers
  name: (identifier) @name) @definition.function

; ── Constructors ──
(constructor_declaration
  (modifiers)? @modifiers
  name: (identifier) @name) @definition.function
