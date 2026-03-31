; Scala import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the import path identifier
;
; Scala imports like `import scala.collection.mutable.Map` are parsed as
; `import_declaration` with multiple `path` fields: each segment is a
; separate `identifier` node with `.` separators. We capture each
; `identifier` in the path, and the engine derives the imported name
; from the last captured segment.

; Import path segments: import foo or import scala.collection.mutable.Map
(import_declaration
  path: (identifier) @module)
