; Haskell symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name              — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;
; Haskell visibility: all public by default (module export lists control
; visibility but are not per-definition). Handled in resolve_visibility().
;
; NOTE: tree-sitter-haskell uses `variable` for function names (lowercase)
; and `name` for type/class names (uppercase).

; ── Functions (top-level bindings) ──
(function
  name: (variable) @name) @definition.function

; ── Data types: data Foo = ... ──
(data_type
  name: (name) @name) @definition.struct

; ── Newtypes: newtype Foo = ... ──
(newtype
  name: (name) @name) @definition.struct

; ── Type classes: class Monad m where ... ──
(class
  name: (name) @name) @definition.trait

; ── Type synonyms: type Foo = ... ──
; NOTE: the node type has a typo in the grammar: `type_synomym`
(type_synomym
  name: (name) @name) @definition.type_alias
