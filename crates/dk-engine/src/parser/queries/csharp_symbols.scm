; C# symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; C# visibility is resolved in `adjust_symbol` by walking the declaration
; node's `modifier` children. Unlike Java (which has a single `modifiers`
; container node), C# uses `repeat($.modifier)` — each keyword is a
; separate node. Capturing `(modifier)? @modifiers` would produce one
; match per modifier, causing duplicate symbols for multi-modifier
; declarations (e.g. `public static class Foo`).

; ── Classes ──
(class_declaration
  name: (identifier) @name) @definition.class

; ── Interfaces ──
(interface_declaration
  name: (identifier) @name) @definition.interface

; ── Enums ──
(enum_declaration
  name: (identifier) @name) @definition.enum

; ── Structs ──
(struct_declaration
  name: (identifier) @name) @definition.struct

; ── Methods ──
(method_declaration
  name: (identifier) @name) @definition.function

; ── Namespaces ──
; Simple namespace name
(namespace_declaration
  name: (identifier) @name) @definition.module

; Qualified namespace name (e.g. MyApp.Models)
(namespace_declaration
  name: (qualified_name) @name) @definition.module
