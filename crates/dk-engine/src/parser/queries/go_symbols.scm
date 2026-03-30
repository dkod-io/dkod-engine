; Go symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Go visibility is name-based: uppercase first letter = Public, lowercase = Private.
; This is handled in resolve_visibility().

; ── Functions ──
(function_declaration
  name: (identifier) @name) @definition.function

; ── Methods ──
(method_declaration
  name: (field_identifier) @name) @definition.function

; ── Structs ──
(type_declaration
  (type_spec
    name: (type_identifier) @name
    type: (struct_type))) @definition.struct

; ── Interfaces ──
(type_declaration
  (type_spec
    name: (type_identifier) @name
    type: (interface_type))) @definition.interface

; ── Constants ──
(const_declaration
  (const_spec
    name: (identifier) @name)) @definition.const

; ── Package-level variables ──
(var_declaration
  (var_spec
    name: (identifier) @name)) @definition.variable
