; Rust import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module      — the module path prefix (e.g. "std::collections", "crate::auth")
;   @import_name — the imported identifier (e.g. "HashMap", "handler", "utils")

; ── Simple scoped: use path::Name; ──
(use_declaration
  argument: (scoped_identifier
    path: (_) @module
    name: (identifier) @import_name))

; ── Plain identifier: use std; ──
(use_declaration
  argument: (identifier) @module)

; ── Aliased: use path::Name as Alias; ──
(use_declaration
  argument: (use_as_clause
    path: (scoped_identifier
      path: (_) @module
      name: (identifier) @import_name)
    alias: (identifier) @alias))

; ── Grouped: use path::{A, B}; ──
; Each name inside the use_list is captured individually.
(use_declaration
  argument: (scoped_use_list
    path: (_) @module
    list: (use_list
      (identifier) @import_name)))

; ── Glob: use path::*; ──
(use_declaration
  argument: (use_wildcard
    (scoped_identifier
      path: (_) @module)))
