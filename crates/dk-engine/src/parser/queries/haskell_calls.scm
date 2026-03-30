; Haskell call-site extraction queries for QueryDrivenParser.
;
; Haskell uses function application without parentheses (e.g. `map f xs`),
; making it difficult to distinguish calls from other expressions via
; tree-sitter queries alone. We leave this empty — Haskell call extraction
; would require more sophisticated analysis.
