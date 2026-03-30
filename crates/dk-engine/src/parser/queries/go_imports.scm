; Go import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the import path (interpreted_string_literal, quotes stripped by engine)
;   @alias  — optional package alias (package_identifier)
;
; Go imports use the last segment of the path as the imported name by default.
; The engine's fallback logic (rsplit on '/') handles this automatically.
;
; Uses `(package_identifier)?` to match both aliased and non-aliased imports
; in a single pattern, avoiding duplicate matches.

; ── Single import: import "fmt" or import alias "path/to/pkg" ──
(import_declaration
  (import_spec
    (package_identifier)? @alias
    path: (interpreted_string_literal) @module))

; ── Grouped imports: import ( "fmt" \n alias "path/to/pkg" ) ──
(import_declaration
  (import_spec_list
    (import_spec
      (package_identifier)? @alias
      path: (interpreted_string_literal) @module)))
