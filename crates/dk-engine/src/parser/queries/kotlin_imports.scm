; Kotlin import extraction queries for QueryDrivenParser.
;
; Captures:
;   @module — the imported package/class identifier
;
; Kotlin uses `import com.example.Foo` which is parsed as
; `import_header` > `identifier` > `simple_identifier` children.
; We capture the `identifier` node (the full dotted path).

; ── import com.example.Foo ──
(import_header
  (identifier) @module)
