; Bash import extraction queries for QueryDrivenParser.
;
; Bash uses `source` or `.` to include other scripts. These are parsed as
; regular commands, making them hard to distinguish from other calls via
; queries alone. We leave this empty — source/dot imports would need
; special handling beyond tree-sitter queries.
