; Local queries against victorhqc/tree-sitter-prisma 1.6.0 (MIT).
; Models, views and embedded composite types are structural data units.
(model_declaration (identifier) @name) @definition.struct
(view_declaration (identifier) @name) @definition.struct
(type_declaration (identifier) @name) @definition.type
(enum_declaration (identifier) @name) @definition.enum
(column_declaration (identifier) @name (column_type)) @definition.field
(enumeral (identifier) @name) @definition.enum_member
(string) @text
