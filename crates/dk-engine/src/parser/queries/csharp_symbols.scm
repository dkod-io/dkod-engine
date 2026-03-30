; C# symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;   @modifiers         — optional modifiers (e.g. "public", "private", "static")
;
; C# visibility: check @modifiers for public/private/protected/internal keywords.
; Default (no modifier) is private.
;
; The `(modifier)?` syntax produces a single match per node regardless
; of whether modifiers are present.

; ── Classes ──
(class_declaration
  (modifier)? @modifiers
  name: (identifier) @name) @definition.class

; ── Interfaces ──
(interface_declaration
  (modifier)? @modifiers
  name: (identifier) @name) @definition.interface

; ── Enums ──
(enum_declaration
  (modifier)? @modifiers
  name: (identifier) @name) @definition.enum

; ── Structs ──
(struct_declaration
  (modifier)? @modifiers
  name: (identifier) @name) @definition.struct

; ── Methods ──
(method_declaration
  (modifier)? @modifiers
  name: (identifier) @name) @definition.function

; ── Namespaces ──
; Simple namespace name
(namespace_declaration
  name: (identifier) @name) @definition.module

; Qualified namespace name (e.g. MyApp.Models)
(namespace_declaration
  name: (qualified_name) @name) @definition.module
