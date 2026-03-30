; Java import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the full import path (e.g. "java.util.List")
;
; Java imports are always fully qualified paths. The engine's fallback
; logic derives the imported name from the last segment (rsplit on '.').
;
; Static imports (import static ...) are also captured since the AST
; structure is the same.

; ── Regular and static imports ──
; import java.util.List;
; import static java.lang.Math.PI;
(import_declaration
  (scoped_identifier) @module)
