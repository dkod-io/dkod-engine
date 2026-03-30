; Python import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module      — the module path (e.g. "os", "os.path", ".local_module")
;   @import_name — the imported identifier (e.g. "os", "join", "helper")
;
; For `import X` the module_path and imported_name are both X.
; For `from X import Y` the module_path is X and imported_name is Y.
; Relative imports (from .foo / from ..bar) include dots in module_path.
;
; NOTE: In tree-sitter-python 0.23, the `name` field of both
; `import_statement` and `import_from_statement` only accepts
; `dotted_name` or `aliased_import` — never bare `identifier`.

; ── import X ──
; e.g. `import os`, `import sys`
(import_statement
  name: (dotted_name) @module)

; ── from X import Y (dotted_name module) ──
; e.g. `from os.path import join`, `from collections import OrderedDict`
(import_from_statement
  module_name: (dotted_name) @module
  name: (dotted_name) @import_name)

; ── from .relative import Y ──
; e.g. `from .local_module import helper`, `from ..parent import utils`
(import_from_statement
  module_name: (relative_import) @module
  name: (dotted_name) @import_name)

; ── import X as Y ──
; e.g. `import numpy as np`, `import tensorflow as tf`
(import_statement
  name: (aliased_import
    name: (dotted_name) @module
    alias: (identifier) @alias))

; ── from X import Y as Z (dotted_name module) ──
; e.g. `from os.path import join as j`
(import_from_statement
  module_name: (dotted_name) @module
  name: (aliased_import
    name: (dotted_name) @import_name
    alias: (identifier) @alias))

; ── from .relative import Y as Z ──
(import_from_statement
  module_name: (relative_import) @module
  name: (aliased_import
    name: (dotted_name) @import_name
    alias: (identifier) @alias))
