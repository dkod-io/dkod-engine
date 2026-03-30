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
(import_statement
  (import_clause
    (namespace_import
      (identifier) @alias))
  source: (string) @module)

; ── Side-effect import: import 'module' ──
(import_statement
  source: (string) @module)
