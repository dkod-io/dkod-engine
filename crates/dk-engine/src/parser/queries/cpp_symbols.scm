; C++ symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; C++ visibility: top-level symbols default to Public.
; Class member visibility (public/private/protected sections) is not parsed
; since we focus on top-level declarations.

; ── Functions ──
; function_definition has declarator: field containing a function_declarator
; which has its own declarator: field for the actual name.
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @name)) @definition.function

; Qualified function definitions: void MyClass::method() { }
(function_definition
  declarator: (function_declarator
    declarator: (qualified_identifier
      name: (identifier) @name))) @definition.function

; ── Classes ──
(class_specifier
  name: (type_identifier) @name) @definition.class

; ── Structs ──
(struct_specifier
  name: (type_identifier) @name) @definition.struct

; ── Enums ──
(enum_specifier
  name: (type_identifier) @name) @definition.enum

; ── Namespaces ──
(namespace_definition
  name: (namespace_identifier) @name) @definition.module
