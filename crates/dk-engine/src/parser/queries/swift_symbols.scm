; Swift symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;   @modifiers         — optional visibility modifiers
;
; Swift visibility: check @modifiers for public/private/internal/fileprivate/open.
; Default (no modifier) is internal, which we map to Private.
;
; NOTE: tree-sitter-swift 0.6 represents enums, structs, and classes all as
; `class_declaration`. We capture them all as `definition.class` and then
; use `adjust_symbol` to refine the kind based on the body type
; (enum_class_body → Enum, class_body → Class/Struct).

; ── Classes / Structs / Enums (all class_declaration in the AST) ──
(class_declaration
  (modifiers
    (visibility_modifier)? @modifiers)?
  name: (type_identifier) @name) @definition.class

; ── Protocols ──
(protocol_declaration
  (modifiers
    (visibility_modifier)? @modifiers)?
  name: (type_identifier) @name) @definition.interface

; ── Functions ──
(function_declaration
  (modifiers
    (visibility_modifier)? @modifiers)?
  name: (simple_identifier) @name) @definition.function
