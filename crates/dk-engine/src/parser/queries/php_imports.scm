; PHP import (use) extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the imported namespace/class path
;
; PHP uses `use Foo\Bar\Baz;` for imports. These are parsed as
; `namespace_use_declaration` nodes containing `namespace_use_clause`
; children with `qualified_name` or `name` children.
;
; The engine's fallback logic derives the imported name from the last
; segment (rsplit on '\').

; ── use Foo\Bar\Baz; ──
(namespace_use_declaration
  (namespace_use_clause
    (qualified_name) @module))
