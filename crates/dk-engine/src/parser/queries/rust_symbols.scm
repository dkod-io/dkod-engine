; Rust symbol extraction queries for QueryDrivenParser.
;
; Each pattern captures:
;   @name             — the symbol identifier
;   @definition.<kind> — the entire node (used for span, signature, doc comments)
;   @modifiers        — optional visibility_modifier text (e.g. "pub", "pub(crate)")
;
; Alternation groups [ ... ] ensure each node matches exactly once:
; the first alternative captures @modifiers when a visibility_modifier is present,
; the second matches nodes without one.

; ── Functions ──
[
  (function_item
    (visibility_modifier) @modifiers
    name: (identifier) @name) @definition.function
  (function_item
    name: (identifier) @name) @definition.function
]

; ── Structs ──
[
  (struct_item
    (visibility_modifier) @modifiers
    name: (type_identifier) @name) @definition.struct
  (struct_item
    name: (type_identifier) @name) @definition.struct
]

; ── Enums ──
[
  (enum_item
    (visibility_modifier) @modifiers
    name: (type_identifier) @name) @definition.enum
  (enum_item
    name: (type_identifier) @name) @definition.enum
]

; ── Traits ──
[
  (trait_item
    (visibility_modifier) @modifiers
    name: (type_identifier) @name) @definition.trait
  (trait_item
    name: (type_identifier) @name) @definition.trait
]

; ── Impl blocks ──
; impl items cannot have visibility modifiers. The @name capture uses the
; `type` field (the implemented type); adjust_symbol() constructs the full
; "impl Trait for Type" or "impl Type" name from the AST node.
(impl_item
  type: (_) @name) @definition.impl

; ── Type aliases ──
[
  (type_item
    (visibility_modifier) @modifiers
    name: (type_identifier) @name) @definition.type_alias
  (type_item
    name: (type_identifier) @name) @definition.type_alias
]

; ── Constants ──
[
  (const_item
    (visibility_modifier) @modifiers
    name: (identifier) @name) @definition.const
  (const_item
    name: (identifier) @name) @definition.const
]

; ── Statics ──
[
  (static_item
    (visibility_modifier) @modifiers
    name: (identifier) @name) @definition.static
  (static_item
    name: (identifier) @name) @definition.static
]

; ── Modules ──
[
  (mod_item
    (visibility_modifier) @modifiers
    name: (identifier) @name) @definition.module
  (mod_item
    name: (identifier) @name) @definition.module
]
