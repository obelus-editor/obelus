; Swift's definitions, for the outline.
;
; Obelus's own rather than the grammar's `TAGS_QUERY`, which captures a
; class's methods, initialisers and properties as `definition.*` on the
; *whole class* -- so every member encloses the next and the outline steps
; one deeper a row -- and every `let` in a function body as a property.
; Code navigation reads only `@name` and never notices; an outline reads
; the extent too. What counts as a definition here is what the grammar's
; own `outline.scm` takes, which is written for Zed's format and not this
; one.

; `extension Foo` names a type it did not write, which the grammar calls a
; `user_type` rather than a `type_identifier`.
(class_declaration
  name: [(type_identifier) (user_type)] @name) @definition.class

(protocol_declaration
  name: (type_identifier) @name) @definition.interface

; Wherever it is: at the top of a file, in a type, or inside another
; function, which is a definition the reader may want to find too. A
; `simple_identifier` and not anything under `name:`, because the grammar
; puts the return type under that field as well.
(function_declaration
  name: (simple_identifier) @name) @definition.function

(init_declaration
  name: "init" @name) @definition.method

(deinit_declaration
  "deinit" @name) @definition.method

(subscript_declaration
  "subscript" @name) @definition.method

(protocol_function_declaration
  name: (simple_identifier) @name) @definition.method

(protocol_property_declaration
  name: (pattern (simple_identifier) @name)) @definition.property

; A type's properties, and not a function's locals: in this grammar a
; `let` in a body is a `property_declaration` as well, so the parent is
; the only thing that tells them apart.
(class_body
  (property_declaration
    name: (pattern (simple_identifier) @name)) @definition.property)

(enum_class_body
  (property_declaration
    name: (pattern (simple_identifier) @name)) @definition.property)
