; Haskell import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the imported module path
;
; Haskell uses `import Data.List` or `import qualified Data.Map as Map`.
; The `import` node has a `module` field containing a `module` node with
; `module_id` children (the dotted segments). We capture the entire
; `module` node and let the engine extract the text.

; ── import Module.Path ──
(import
  module: (module) @module)
