; Ruby import (require) extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the required file/gem path (string content)
;
; Ruby uses `require 'foo'` and `require_relative 'bar'` for imports.
; These are parsed as `call` nodes with method name "require" or
; "require_relative".
;
; The engine's fallback logic derives the imported name from the last
; segment (rsplit on '/').

; ── require 'foo' ──
(call
  method: (identifier) @_method
  arguments: (argument_list
    (string
      (string_content) @module))
  (#eq? @_method "require"))

; ── require_relative 'bar' ──
(call
  method: (identifier) @_method
  arguments: (argument_list
    (string
      (string_content) @module))
  (#eq? @_method "require_relative"))
