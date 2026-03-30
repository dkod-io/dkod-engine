; C++ include extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the include path (system_lib_string or string_literal)
;
; #include <iostream>  → system_lib_string
; #include "myheader.h" → string_literal
;
; The engine's fallback logic derives the imported name from the last
; path segment (rsplit on '/').

; ── System includes: #include <iostream> ──
(preproc_include
  path: (system_lib_string) @module)

; ── Local includes: #include "myheader.h" ──
(preproc_include
  path: (string_literal) @module)
