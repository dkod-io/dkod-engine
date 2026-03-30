; TypeScript/JavaScript symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;   @modifiers         — optional, captured when the declaration is wrapped in
;                        an export_statement (indicates Public visibility)
;
; Visibility: declarations wrapped in `export_statement` are Public, all
; others are Private. The @modifiers capture is the `export_statement` itself.

; ── Exported function ──
(export_statement
  (function_declaration
    name: (identifier) @name) @definition.function) @modifiers

; ── Non-exported function ──
(function_declaration
  name: (identifier) @name) @definition.function

; ── Exported class ──
(export_statement
  (class_declaration
    name: (type_identifier) @name) @definition.class) @modifiers

; ── Non-exported class ──
(class_declaration
  name: (type_identifier) @name) @definition.class

; ── Exported interface ──
(export_statement
  (interface_declaration
    name: (type_identifier) @name) @definition.interface) @modifiers

; ── Non-exported interface ──
(interface_declaration
  name: (type_identifier) @name) @definition.interface

; ── Exported type alias ──
(export_statement
  (type_alias_declaration
    name: (type_identifier) @name) @definition.type_alias) @modifiers

; ── Non-exported type alias ──
(type_alias_declaration
  name: (type_identifier) @name) @definition.type_alias

; ── Exported enum ──
(export_statement
  (enum_declaration
    name: (identifier) @name) @definition.enum) @modifiers

; ── Non-exported enum ──
(enum_declaration
  name: (identifier) @name) @definition.enum

; ── Exported const/let/var ──
(export_statement
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name)) @definition.const) @modifiers

; ── Non-exported const/let/var ──
(lexical_declaration
  (variable_declarator
    name: (identifier) @name)) @definition.const

; ── Exported default identifier (e.g. `export default router;`) ──
(export_statement
  value: (identifier) @name) @definition.expression

; ── Expression statement with method call (e.g. `router.get("/path", ...)`) ──
(expression_statement
  (call_expression
    function: (member_expression) @name) @definition.expression)

; ── Expression statement with direct call (e.g. `app(middleware)`) ──
(expression_statement
  (call_expression
    function: (identifier) @name) @definition.expression)

; ── Expression statement with assignment (e.g. `module.exports = ...`) ──
(expression_statement
  (assignment_expression
    left: (_) @name) @definition.expression)
