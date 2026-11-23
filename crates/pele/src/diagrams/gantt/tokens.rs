//! gantt の字句の種類（pele の `src/diagrams/gantt/tokens.ts`）。

#![allow(non_upper_case_globals)]

pub const END: u8 = 0;
pub const ERROR: u8 = 1;
pub const open_directive: u8 = 2;
pub const acc_title: u8 = 3;
pub const acc_title_value: u8 = 4;
pub const acc_descr: u8 = 5;
pub const acc_descr_value: u8 = 6;
pub const acc_descr_multiline_value: u8 = 7;
pub const NL: u8 = 8;
pub const href: u8 = 9;
pub const callbackname: u8 = 10;
pub const callbackargs: u8 = 11;
pub const click: u8 = 12;
pub const gantt: u8 = 13;
pub const dateFormat: u8 = 14;
pub const inclusiveEndDates: u8 = 15;
pub const topAxis: u8 = 16;
pub const axisFormat: u8 = 17;
pub const tickInterval: u8 = 18;
pub const includes: u8 = 19;
pub const excludes: u8 = 20;
pub const todayMarker: u8 = 21;
pub const weekday_monday: u8 = 22;
pub const weekday_tuesday: u8 = 23;
pub const weekday_wednesday: u8 = 24;
pub const weekday_thursday: u8 = 25;
pub const weekday_friday: u8 = 26;
pub const weekday_saturday: u8 = 27;
pub const weekday_sunday: u8 = 28;
pub const weekend_friday: u8 = 29;
pub const weekend_saturday: u8 = 30;
pub const date: u8 = 31;
pub const title: u8 = 32;
pub const accDescription: u8 = 33;
pub const section: u8 = 34;
pub const taskTxt: u8 = 35;
pub const taskData: u8 = 36;
pub const COLON: u8 = 37;
pub const EOF: u8 = 38;

pub const TOKEN_NAMES: [&str; 39] = [
    "$end",
    "INVALID",
    "open_directive",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "NL",
    "href",
    "callbackname",
    "callbackargs",
    "click",
    "gantt",
    "dateFormat",
    "inclusiveEndDates",
    "topAxis",
    "axisFormat",
    "tickInterval",
    "includes",
    "excludes",
    "todayMarker",
    "weekday_monday",
    "weekday_tuesday",
    "weekday_wednesday",
    "weekday_thursday",
    "weekday_friday",
    "weekday_saturday",
    "weekday_sunday",
    "weekend_friday",
    "weekend_saturday",
    "date",
    "title",
    "accDescription",
    "section",
    "taskTxt",
    "taskData",
    ":",
    "EOF",
];
