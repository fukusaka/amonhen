; Kotlin の色付け（tree-sitter-grammars/tree-sitter-kotlin 向け。この文法はクエリを同梱しないので、ここで書く）。
; キャプチャ名はほかの文法と同じ慣例（lib.rs の NAMES）。同じノードに複数の決まりが合うときは、後に書いた方が勝つ。

(line_comment) @comment
(block_comment) @comment
(shebang) @comment

(string_literal) @string
(multiline_string_literal) @string
(character_literal) @string
(escape_sequence) @string.escape

(number_literal) @number
(float_literal) @number

; true・false・null は identifier として現れる。大文字と _ だけの名前は定数とみなす。
((identifier) @constant
  (#any-of? @constant "true" "false" "null"))
((identifier) @constant
  (#match? @constant "^[A-Z][A-Z0-9_]+$"))

(annotation) @attribute
(file_annotation) @attribute
(use_site_target) @attribute
(label) @label

; 宣言
(function_declaration name: (identifier) @function)
(class_declaration name: (identifier) @type)
(object_declaration name: (identifier) @type)
(type_alias type: (identifier) @type)
(type_parameter (identifier) @type)
(enum_entry (identifier) @constant)
(user_type (identifier) @type)
(parameter (identifier) @variable.parameter)
(class_parameter (identifier) @variable.parameter)
(lambda_parameters (variable_declaration (identifier) @variable.parameter))

; a.b は property、呼び出し a.b(...) は function（同じノードには後に書いた決まりが勝つので、呼び出しを後に）
(navigation_expression (identifier) @property .)
(call_expression (identifier) @function)
(call_expression (navigation_expression (identifier) @function .))
(callable_reference (identifier) @function .)

; アノテーションの名前は型でなく attribute（user_type の決まりより後に）
(annotation (user_type (identifier) @attribute))
(annotation (constructor_invocation (user_type (identifier) @attribute)))

(package_header (qualified_identifier) @module)
(import (qualified_identifier) @module)

[
  "abstract" "actual" "annotation" "as" "by" "catch" "class" "companion" "const" "constructor" "crossinline"
  "data" "delegate" "do" "dynamic" "else" "enum" "expect" "external" "field" "file" "final" "finally" "for"
  "fun" "get" "if" "import" "in" "infix" "init" "inline" "inner" "interface" "internal" "is" "lateinit"
  "noinline" "object" "open" "operator" "out" "override" "package" "param" "private" "property" "protected"
  "public" "receiver" "return" "sealed" "set" "setparam" "super" "suspend" "tailrec" "this" "throw" "try"
  "typealias" "val" "value" "var" "vararg" "when" "where" "while"
  "as?" "return@" "super@" "this@" "!in" "!is"
] @keyword

[
  "!" "!!" "!=" "!==" "%" "%=" "&&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "->" ".." "..<" "/" "/="
  "<" "<=" "=" "==" "===" ">" ">=" "?" "?." "?:" "||" "::"
] @operator
