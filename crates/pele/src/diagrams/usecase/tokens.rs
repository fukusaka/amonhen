//! use case の字句の種類（pele の `src/diagrams/usecase/tokens.ts`）。Mermaid の字句の種類を、その分類が範囲に
//! なる順に: WORD は USECASE から IDENTIFIER、LABEL_TEXT は USECASE から LABEL_SYMBOL。

pub const EOF: u8 = 0;
pub const USECASE: u8 = 1;
pub const ACTOR: u8 = 2;
pub const SYSTEM_BOUNDARY: u8 = 3;
pub const END: u8 = 4;
pub const DIRECTION: u8 = 5;
pub const TD: u8 = 6;
pub const TB: u8 = 7;
pub const BT: u8 = 8;
pub const LR: u8 = 9;
pub const RL: u8 = 10;
pub const NOTE: u8 = 11;
pub const FOR: u8 = 12;
pub const JSON: u8 = 13;
pub const CLASS_DEF: u8 = 14;
pub const CLASS: u8 = 15;
pub const STYLE: u8 = 16;
pub const INCLUDE: u8 = 17;
pub const EXTEND: u8 = 18;
pub const TRUE: u8 = 19;
pub const FALSE: u8 = 20;
pub const IDENTIFIER: u8 = 21;
pub const NUMBER: u8 = 22;
pub const AT: u8 = 23;
pub const COMMA: u8 = 24;
pub const COLON: u8 = 25;
pub const HASH_COLOR: u8 = 26;
pub const CSS_IDENTIFIER: u8 = 27;
pub const CSS_ESCAPED_COMMA: u8 = 28;
pub const DASH: u8 = 29;
pub const DOT: u8 = 30;
pub const PERCENT: u8 = 31;
pub const CSS_PUNCTUATION: u8 = 32;
pub const LABEL_PUNCTUATION: u8 = 33;
pub const LABEL_SYMBOL: u8 = 34;
pub const PLAIN_STRING: u8 = 35;
pub const MARKDOWN_STRING: u8 = 36;
pub const UNCLOSED_MARKDOWN_STRING: u8 = 37;
pub const COMMENT: u8 = 38;
pub const NEWLINE: u8 = 39;
pub const ACC_DESCR_BLOCK: u8 = 40;
pub const ACC_TITLE_LINE: u8 = 41;
pub const ACC_DESCR_LINE: u8 = 42;
pub const JSON_DECLARATION_START: u8 = 43;
pub const JSON_OBJECT_LITERAL: u8 = 44;
pub const UNCLOSED_JSON_OBJECT_LITERAL: u8 = 45;
pub const GENERALIZATION: u8 = 46;
pub const DEPENDENCY_ARROW: u8 = 47;
pub const STEREOTYPE_START: u8 = 48;
pub const STEREOTYPE_END: u8 = 49;
pub const STEREOTYPE_TEXT: u8 = 50;
pub const UNCLOSED_STEREOTYPE_TEXT: u8 = 51;
pub const CLASS_SEPARATOR: u8 = 52;
pub const FORWARD_SOLID: u8 = 53;
pub const BACKWARD_SOLID: u8 = 54;
pub const FORWARD_CIRCLE: u8 = 55;
pub const BACKWARD_CIRCLE: u8 = 56;
pub const FORWARD_CROSS: u8 = 57;
pub const BACKWARD_CROSS: u8 = 58;
pub const MARKERLESS_SOLID: u8 = 59;
pub const METADATA_START: u8 = 60;
pub const LBRACE: u8 = 61;
pub const RBRACE: u8 = 62;
pub const LBRACKET: u8 = 63;
pub const RBRACKET: u8 = 64;
pub const LPAREN: u8 = 65;
pub const RPAREN: u8 = 66;

pub const TOKEN_NAMES: &[&str] = &[
    "EOF",
    "USECASE",
    "ACTOR",
    "SYSTEM_BOUNDARY",
    "END",
    "DIRECTION",
    "TD",
    "TB",
    "BT",
    "LR",
    "RL",
    "NOTE",
    "FOR",
    "JSON",
    "CLASS_DEF",
    "CLASS",
    "STYLE",
    "INCLUDE",
    "EXTEND",
    "TRUE",
    "FALSE",
    "IDENTIFIER",
    "NUMBER",
    "AT",
    "COMMA",
    "COLON",
    "HASH_COLOR",
    "CSS_IDENTIFIER",
    "CSS_ESCAPED_COMMA",
    "DASH",
    "DOT",
    "PERCENT",
    "CSS_PUNCTUATION",
    "LABEL_PUNCTUATION",
    "LABEL_SYMBOL",
    "PLAIN_STRING",
    "MARKDOWN_STRING",
    "UNCLOSED_MARKDOWN_STRING",
    "COMMENT",
    "NEWLINE",
    "ACC_DESCR_BLOCK",
    "ACC_TITLE_LINE",
    "ACC_DESCR_LINE",
    "JSON_DECLARATION_START",
    "JSON_OBJECT_LITERAL",
    "UNCLOSED_JSON_OBJECT_LITERAL",
    "GENERALIZATION",
    "DEPENDENCY_ARROW",
    "STEREOTYPE_START",
    "STEREOTYPE_END",
    "STEREOTYPE_TEXT",
    "UNCLOSED_STEREOTYPE_TEXT",
    "CLASS_SEPARATOR",
    "FORWARD_SOLID",
    "BACKWARD_SOLID",
    "FORWARD_CIRCLE",
    "BACKWARD_CIRCLE",
    "FORWARD_CROSS",
    "BACKWARD_CROSS",
    "MARKERLESS_SOLID",
    "METADATA_START",
    "LBRACE",
    "RBRACE",
    "LBRACKET",
    "RBRACKET",
    "LPAREN",
    "RPAREN",
];
