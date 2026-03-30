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
;
; Alternation groups `[...]` ensure each node produces exactly one match
; (first matching alternative wins), preventing spurious duplicates when
; a declaration is both exported and matches the non-exported pattern.

; ── Functions ──
[
  (export_statement
    (function_declaration
      name: (identifier) @name) @definition.function) @modifiers
  (function_declaration
    name: (identifier) @name) @definition.function
]

; ── Classes ──
[
  (export_statement
    (class_declaration
      name: (type_identifier) @name) @definition.class) @modifiers
  (class_declaration
    name: (type_identifier) @name) @definition.class
]

; ── Interfaces ──
[
  (export_statement
    (interface_declaration
      name: (type_identifier) @name) @definition.interface) @modifiers
  (interface_declaration
    name: (type_identifier) @name) @definition.interface
]

; ── Type aliases ──
[
  (export_statement
    (type_alias_declaration
      name: (type_identifier) @name) @definition.type_alias) @modifiers
  (type_alias_declaration
    name: (type_identifier) @name) @definition.type_alias
]

; ── Enums ──
[
  (export_statement
    (enum_declaration
      name: (identifier) @name) @definition.enum) @modifiers
  (enum_declaration
    name: (identifier) @name) @definition.enum
]

; ── const/let/var ──
[
  (export_statement
    (lexical_declaration
      (variable_declarator
        name: (identifier) @name)) @definition.const) @modifiers
  (lexical_declaration
    (variable_declarator
      name: (identifier) @name)) @definition.const
]

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
