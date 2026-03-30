; Swift import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the imported module name
;
; Swift uses `import Foundation`, `import UIKit`, etc.
; The import path is an `identifier` node containing `simple_identifier` children.

; ── import Foundation ──
(import_declaration
  (identifier
    (simple_identifier) @module))
