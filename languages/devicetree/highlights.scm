[
    "/delete-node/"
    "/delete-property/"
    "/dts-v1/"
    "/incbin/"
    "/include/"
    "/memreserve/"
    "/omit-if-no-ref/"
    "#define"
    "#undef"
    "#include"
    "#if"
    "#elif"
    "#else"
    "#endif"
    "#ifdef"
    "#ifndef"
    "/plugin/"
] @keyword

[
    "!"
    "~"
    "-"
    "+"
    "*"
    "/"
    "%"
    "||"
    "&&"
    "|"
    "^"
    "&"
    "=="
    "!="
    ">"
    ">="
    "<="
    ">"
    "<<"
    ">>"
] @operator

[
    ","
    ";"
] @punctuation.delimiter

[
    "("
    ")"
    "{"
    "}"
    "<"
    ">"
] @punctuation.bracket

(string_literal) @string
(integer_literal) @number

(call_expression
    function: (identifier) @function)

(node
    name: (identifier) @label)

(node
    label: (identifier) @variable.special)

(property
    label: (identifier) @label)

(memory_reservation
    label: (identifier) @label)

(property
    name: (identifier) @property)

(property
    value: (string_literal) @string)

(integer_cells
    (identifier) @number)

(property
    value: (integer_cells) @number)

(preproc_include
    path: (string_literal) @string)

(preproc_include
    path: (system_lib_string) @string)

(unit_address) @tag

(reference) @constant

(comment) @comment
