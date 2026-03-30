; C# import (using) extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the imported namespace (identifier or qualified_name)
;
; C# uses `using` directives to import namespaces.
; The engine's fallback logic derives the imported name from the last
; segment (rsplit on '.').

; ── Simple using: using System; ──
(using_directive
  (identifier) @module)

; ── Qualified using: using System.Collections.Generic; ──
(using_directive
  (qualified_name) @module)
