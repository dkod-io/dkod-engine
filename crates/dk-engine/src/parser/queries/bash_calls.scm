; Bash call-site extraction queries for QueryDrivenParser.
;
; Captures:
;   @callee — the called command name
;   @call   — the entire command node (used for span)
;
; Bash commands are calls. The command_name child contains a word (or
; expansion) for the command being invoked.

; ── Command calls: git status, echo "hello" ──
(command
  name: (command_name
    (word) @callee)) @call
