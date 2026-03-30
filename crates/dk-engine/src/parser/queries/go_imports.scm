; Go import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the import path (interpreted_string_literal, quotes stripped by engine)
;   @alias  — optional package alias (package_identifier)
;
; Go imports use the last segment of the path as the imported name by default.
; The engine's fallback logic (rsplit on '/') handles this automatically.

; ── Single import: import "fmt" ──
(import_declaration
  (import_spec
    path: (interpreted_string_literal) @module))

; ── Aliased import: import alias "path/to/pkg" ──
(import_declaration
  (import_spec
    name: (package_identifier) @alias
    path: (interpreted_string_literal) @module))

; ── Grouped imports: import ( "fmt" \n "net/http" ) ──
(import_declaration
  (import_spec_list
    (import_spec
      path: (interpreted_string_literal) @module)))

; ── Grouped aliased imports ──
(import_declaration
  (import_spec_list
    (import_spec
      name: (package_identifier) @alias
      path: (interpreted_string_literal) @module)))
