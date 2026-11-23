//! agentflow の字句の種類（pele の `src/diagrams/agentflow/tokens.ts`）。

#![allow(non_upper_case_globals)]

pub const END: u8 = 0;
pub const ERROR: u8 = 1;
pub const acc_title: u8 = 2;
pub const acc_title_value: u8 = 3;
pub const acc_descr: u8 = 4;
pub const acc_descr_value: u8 = 5;
pub const acc_descr_multiline_value: u8 = 6;
pub const SHAPE_DATA: u8 = 7;
pub const CALLBACKNAME: u8 = 8;
pub const CALLBACKARGS: u8 = 9;
pub const MD_STR: u8 = 10;
pub const STR: u8 = 11;
pub const STYLE: u8 = 12;
pub const DEFAULT: u8 = 13;
pub const LINKSTYLE: u8 = 14;
pub const INTERPOLATE: u8 = 15;
pub const CLASSDEF: u8 = 16;
pub const CLASS: u8 = 17;
pub const HREF: u8 = 18;
pub const CLICK: u8 = 19;
pub const GRAPH: u8 = 20;
pub const flow: u8 = 21;
pub const connector: u8 = 22;
pub const global: u8 = 23;
pub const end: u8 = 24;
pub const LINK_TARGET: u8 = 25;
pub const NODIR: u8 = 26;
pub const DIR: u8 = 27;
pub const direction_tb: u8 = 28;
pub const direction_bt: u8 = 29;
pub const direction_rl: u8 = 30;
pub const direction_lr: u8 = 31;
pub const direction_td: u8 = 32;
pub const LINK_ID: u8 = 33;
pub const NUM: u8 = 34;
pub const BRKT: u8 = 35;
pub const STYLE_SEPARATOR: u8 = 36;
pub const COLON: u8 = 37;
pub const AMP: u8 = 38;
pub const SEMI: u8 = 39;
pub const COMMA: u8 = 40;
pub const MULT: u8 = 41;
pub const LINK: u8 = 42;
pub const START_LINK: u8 = 43;
pub const EDGE_TEXT: u8 = 44;
pub const ELLIPSE_END: u8 = 45;
pub const TEXT: u8 = 46;
pub const ELLIPSE_START: u8 = 47;
pub const STADIUMEND: u8 = 48;
pub const STADIUMSTART: u8 = 49;
pub const SUBROUTINEEND: u8 = 50;
pub const SUBROUTINESTART: u8 = 51;
pub const VERTEX_WITH_PROPS_START: u8 = 52;
pub const TAGEND: u8 = 53;
pub const CYLINDEREND: u8 = 54;
pub const CYLINDERSTART: u8 = 55;
pub const DOUBLECIRCLEEND: u8 = 56;
pub const DOUBLECIRCLESTART: u8 = 57;
pub const TRAPEND: u8 = 58;
pub const INVTRAPEND: u8 = 59;
pub const TRAPSTART: u8 = 60;
pub const INVTRAPSTART: u8 = 61;
pub const TAGSTART: u8 = 62;
pub const UP: u8 = 63;
pub const SEP: u8 = 64;
pub const DOWN: u8 = 65;
pub const COMMENT: u8 = 66;
pub const NODE_STRING: u8 = 67;
pub const MINUS: u8 = 68;
pub const UNICODE_TEXT: u8 = 69;
pub const PIPE: u8 = 70;
pub const PE: u8 = 71;
pub const PS: u8 = 72;
pub const SQE: u8 = 73;
pub const SQS: u8 = 74;
pub const DIAMOND_STOP: u8 = 75;
pub const DIAMOND_START: u8 = 76;
pub const NEWLINE: u8 = 77;
pub const SPACE: u8 = 78;
pub const EOF: u8 = 79;

pub const TOKEN_NAMES: [&str; 80] = [
    "$end",
    "INVALID",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "SHAPE_DATA",
    "CALLBACKNAME",
    "CALLBACKARGS",
    "MD_STR",
    "STR",
    "STYLE",
    "DEFAULT",
    "LINKSTYLE",
    "INTERPOLATE",
    "CLASSDEF",
    "CLASS",
    "HREF",
    "CLICK",
    "GRAPH",
    "flow",
    "connector",
    "global",
    "end",
    "LINK_TARGET",
    "NODIR",
    "DIR",
    "direction_tb",
    "direction_bt",
    "direction_rl",
    "direction_lr",
    "direction_td",
    "LINK_ID",
    "NUM",
    "BRKT",
    "STYLE_SEPARATOR",
    "COLON",
    "AMP",
    "SEMI",
    "COMMA",
    "MULT",
    "LINK",
    "START_LINK",
    "EDGE_TEXT",
    "-)",
    "TEXT",
    "(-",
    "STADIUMEND",
    "STADIUMSTART",
    "SUBROUTINEEND",
    "SUBROUTINESTART",
    "VERTEX_WITH_PROPS_START",
    "TAGEND",
    "CYLINDEREND",
    "CYLINDERSTART",
    "DOUBLECIRCLEEND",
    "DOUBLECIRCLESTART",
    "TRAPEND",
    "INVTRAPEND",
    "TRAPSTART",
    "INVTRAPSTART",
    "TAGSTART",
    "UP",
    "SEP",
    "DOWN",
    "COMMENT",
    "NODE_STRING",
    "MINUS",
    "UNICODE_TEXT",
    "PIPE",
    "PE",
    "PS",
    "SQE",
    "SQS",
    "DIAMOND_STOP",
    "DIAMOND_START",
    "NEWLINE",
    "SPACE",
    "EOF",
];
