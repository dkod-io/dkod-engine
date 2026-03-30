; Julia import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the imported module identifier
;
; Julia uses `import Foo` and `using Foo` for imports. These are parsed
; as `import_statement` / `using_statement` nodes with `identifier` children.

; ── import Foo ──
(import_statement
  (identifier) @module)

; ── using Foo ──
(using_statement
  (identifier) @module)
