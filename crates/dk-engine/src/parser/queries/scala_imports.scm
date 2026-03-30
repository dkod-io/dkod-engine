; Scala import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the import path identifier
;
; Scala imports like `import scala.collection.mutable.Map` are parsed as
; `import_declaration` with a `path` field (first identifier) and nested
; `stable_identifier` children. We capture the `path` field as the module
; since it's the top-level package. The engine's fallback logic handles
; deriving the imported name from the last segment.

; Simple import: import foo
(import_declaration
  path: (identifier) @module)
