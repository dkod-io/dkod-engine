; Kotlin symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Kotlin visibility is resolved in `adjust_symbol` by walking modifier
; children. Default is Public (Kotlin convention).
;
; NOTE: In tree-sitter-kotlin, class/interface/enum are all represented
; as `class_declaration`. The name is `type_identifier` (first child).
; Function names use `simple_identifier`.

; ── Classes / Interfaces / Enums (all use class_declaration) ──
(class_declaration
  (type_identifier) @name) @definition.class

; ── Functions ──
(function_declaration
  (simple_identifier) @name) @definition.function

; ── Objects (singletons) ──
(object_declaration
  (type_identifier) @name) @definition.module

; ── Type aliases ──
(type_alias
  (type_identifier) @name) @definition.type_alias

; ── Properties (val/var) ──
(property_declaration
  (variable_declaration
    (simple_identifier) @name)) @definition.variable
