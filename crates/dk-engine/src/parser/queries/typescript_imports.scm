; TypeScript/JavaScript import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module      — the module path (string literal, quotes stripped by engine)
;   @import_name — the imported identifier
;   @alias       — optional alias for namespace imports (e.g. `* as utils`)
;
; Handles:
;   import { A } from 'module'     → named import
;   import * as ns from 'module'   → namespace import with alias
;   import Default from 'module'   → default import (captured as identifier)

; ── Named imports: import { X } from 'module' ──
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @import_name)))
  source: (string) @module)

; ── Default import: import Foo from 'module' ──
(import_statement
  (import_clause
    (identifier) @import_name)
  source: (string) @module)

; ── Namespace import: import * as ns from 'module' ──
; Capture the alias as both @import_name and @alias so the engine
; doesn't fall back to deriving the name from the module path.
(import_statement
  (import_clause
    (namespace_import
      (identifier) @import_name @alias))
  source: (string) @module)

; Side-effect imports (`import 'polyfill'`) are intentionally not captured.
; They have no named binding, so they don't participate in symbol/conflict
; analysis. Adding a catch-all pattern here would produce duplicates for
; every other import form since tree-sitter field constraints are additive.
