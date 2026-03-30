; Rust import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module      — the module path prefix (e.g. "std::collections", "crate::auth", "super")
;   @import_name — the imported identifier (e.g. "HashMap", "handler", "utils")
;
; Handles simple use declarations of the form:
;   use path::to::Name;

(use_declaration
  argument: (scoped_identifier
    path: (_) @module
    name: (identifier) @import_name))
