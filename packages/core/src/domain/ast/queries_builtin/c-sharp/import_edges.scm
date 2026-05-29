; C# using directives.
; Verified against tree-sitter-c-sharp 0.23.5 node-types.json:
;   using_directive children: (type)  [e.g. qualified_name / identifier]
; Capturing the whole directive keeps the extractor agnostic about the
; `using X = Y;` / `using static X;` variants.
(using_directive) @import
