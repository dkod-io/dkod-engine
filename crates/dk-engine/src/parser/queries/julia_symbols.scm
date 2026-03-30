; Julia symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Julia visibility: names starting with `_` are private by convention,
; everything else is public. Handled in resolve_visibility().

; ── Functions: function foo(x) ... end ──
(function_definition
  (signature
    (call_expression
      .
      (identifier) @name))) @definition.function

; ── Short functions: foo(x) = x + 1 ──
; These are parsed as assignment with a call_expression on the left.
(assignment
  (call_expression
    .
    (identifier) @name)) @definition.function

; ── Structs: struct Foo ... end / mutable struct Foo ... end ──
(struct_definition
  (type_head
    (identifier) @name)) @definition.struct

; ── Abstract types: abstract type Foo end ──
(abstract_definition
  (type_head
    (identifier) @name)) @definition.type_alias

; ── Modules: module Foo ... end ──
(module_definition
  name: (identifier) @name) @definition.module

; ── Macros: macro foo(x) ... end ──
(macro_definition
  (signature
    (call_expression
      .
      (identifier) @name))) @definition.function
