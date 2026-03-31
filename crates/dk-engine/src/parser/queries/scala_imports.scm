; Scala import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the import path identifier
;
; Scala imports like `import scala.collection.mutable.Map` are parsed by
; tree-sitter-scala as an `import_declaration` with multiple `path` fields,
; one per segment: `path: (identifier "scala")`, `path: (identifier
; "collection")`, etc. This query captures each segment individually,
; producing multiple import entries per multi-segment import. The engine
; derives the imported name from the last captured segment (rsplit on '.').
;
; NOTE: `stable_identifier` exists in the grammar but is NOT used in the
; `path` field of `import_declaration` — only `identifier` appears there.

; Import path segments: captures each segment of multi-segment imports
; (e.g., `import scala.collection.mutable.Map` produces 4 captures)
(import_declaration
  path: (identifier) @module)
