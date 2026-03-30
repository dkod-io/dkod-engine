; PHP symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;   @modifiers         — optional visibility modifier (e.g. "public", "private")
;
; PHP visibility: check @modifiers for public/private/protected keywords.
; Default (no modifier) is public (PHP convention).
;
; IMPORTANT: PHP uses `(name)` not `(identifier)` for symbol names,
; and `(visibility_modifier)` not `(modifier)` for visibility.

; ── Classes ──
(class_declaration
  name: (name) @name) @definition.class

; ── Interfaces ──
(interface_declaration
  name: (name) @name) @definition.interface

; ── Enums ──
(enum_declaration
  name: (name) @name) @definition.enum

; ── Methods (with visibility) ──
(method_declaration
  (visibility_modifier)? @modifiers
  name: (name) @name) @definition.function

; ── Standalone functions ──
(function_definition
  name: (name) @name) @definition.function

; ── Namespaces ──
(namespace_definition
  name: (namespace_name) @name) @definition.module
