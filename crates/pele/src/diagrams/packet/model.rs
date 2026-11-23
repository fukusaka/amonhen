//! packet の形（pele の `src/diagrams/packet/model.ts`）。

use super::parser::PacketAst;
use crate::diagrams::info::model::{acc_descr, acc_title, or_title};
use crate::errors::{ErrorCode, PeleError, Result};
use crate::js;
use crate::preprocess::Config;
use crate::util::yaml::Value;

#[derive(Clone, Debug)]
pub struct PacketBlock {
    pub start: f64,
    pub end: f64,
    /// Mermaid の数え方: 行をまたいで分けた欄の切れは、幅より 1 少ない。
    pub bits: f64,
    pub label: String,
}

pub struct PacketModel {
    pub title: Option<String>,
    pub acc_title: Option<String>,
    pub acc_descr: Option<String>,
    pub bits_per_row: f64,
    pub rows: Vec<Vec<PacketBlock>>,
}

const MAX_ROWS: usize = 10_000;

pub fn packet_option(config: &Config, key: &str, fallback: f64, min: f64, max: f64) -> f64 {
    match config
        .get("packet")
        .and_then(Value::as_obj)
        .and_then(|p| p.get(key))
    {
        Some(Value::Num(v)) if *v >= min => js::min(*v, max),
        _ => fallback,
    }
}

fn invalid(message: String) -> PeleError {
    PeleError::typed(message, ErrorCode::Semantic, "packet")
}

pub fn build_packet(ast: PacketAst, config: &Config, title: Option<&str>) -> Result<PacketModel> {
    let bits_per_row = packet_option(config, "bitsPerRow", 32.0, 1.0, 4096.0).floor();
    let mut rows: Vec<Vec<PacketBlock>> = Vec::new();
    let mut last_bit = -1.0;
    let mut word: Vec<PacketBlock> = Vec::new();
    let mut row = 1.0;

    for block in ast.blocks {
        let (start, end, bits) = (block.start, block.end, block.bits);
        if let (Some(s), Some(e)) = (start, end)
            && e < s
        {
            return Err(invalid(format!(
                "Packet block {} - {} is invalid. End must be greater than start.",
                js::fmt(s),
                js::fmt(e)
            )));
        }
        let mut start = start.unwrap_or(last_bit + 1.0);
        if start != last_bit + 1.0 {
            return Err(invalid(format!(
                "Packet block {} - {} is not contiguous. It should start from {}.",
                js::fmt(start),
                js::fmt(end.unwrap_or(start)),
                js::fmt(last_bit + 1.0)
            )));
        }
        if bits == Some(0.0) {
            return Err(invalid(format!(
                "Packet block {} is invalid. Cannot have a zero bit field.",
                js::fmt(start)
            )));
        }
        let end = end.unwrap_or(start + bits.unwrap_or(1.0) - 1.0);
        let mut bits = bits.unwrap_or(end - start + 1.0);
        last_bit = end;

        while (word.len() as f64) <= bits_per_row + 1.0 && rows.len() < MAX_ROWS {
            let row_end = row * bits_per_row - 1.0;
            if end <= row_end {
                word.push(PacketBlock {
                    start,
                    end,
                    bits,
                    label: block.label.clone(),
                });
                if end == row_end {
                    rows.push(std::mem::take(&mut word));
                    row += 1.0;
                }
                break;
            }
            word.push(PacketBlock {
                start,
                end: row_end,
                bits: row_end - start,
                label: block.label.clone(),
            });
            rows.push(std::mem::take(&mut word));
            row += 1.0;
            start = row_end + 1.0;
            bits = end - start;
        }
    }
    if !word.is_empty() {
        rows.push(word);
    }

    Ok(PacketModel {
        title: or_title(ast.title, title),
        acc_title: acc_title(ast.acc_title),
        acc_descr: acc_descr(ast.acc_descr),
        bits_per_row,
        rows,
    })
}
