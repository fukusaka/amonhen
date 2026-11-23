//! ホストの幅に合わせて描き直す（pele の `src/diagrams/common/fit-width.ts`）。

use crate::errors::Result;
use crate::js;
use crate::types::{RenderOptions, Rendered};

fn available(options: &RenderOptions) -> Option<f64> {
    options.max_width.filter(|&a| a > 0.0)
}

/// 図がホストの幅に収まるよう、どんな幅にも描ける図を描く。図のまわり（ラベル、凡例）は描くまで分からないので、
/// はみ出た分を図から引いて描き直す。`narrow` は、自分の幅に収まらなかったことを図に知らせる。
pub fn fit_width(
    options: &RenderOptions,
    natural: f64,
    least: f64,
    mut draw: impl FnMut(f64, bool) -> Result<Rendered>,
) -> Result<Rendered> {
    let mut result = draw(natural, false)?;
    let Some(available) = available(options) else {
        return Ok(result);
    };
    if result.width <= available {
        return Ok(result);
    }
    let floor = js::min(least, natural);
    let mut plot = natural;
    result = draw(plot, true)?;
    let mut round = 0;
    while round < 3 && result.width > available && plot > floor {
        plot = js::max(floor, plot - (result.width - available));
        result = draw(plot, true)?;
        round += 1;
    }
    Ok(result)
}

/// 題が折り返す前に使える幅。
pub fn title_room(options: &RenderOptions) -> f64 {
    match available(options) {
        Some(a) => js::max(80.0, a - 2.0 * options.padding.unwrap_or(8.0)),
        None => 4000.0,
    }
}

/// 決めた幅に描く図の幅: 自分のものか、それより狭ければホストのもの。`around` は図が幅の外に足すもの。
pub fn cap_width(options: &RenderOptions, natural: f64, least: f64, around: f64) -> f64 {
    match available(options) {
        None => natural,
        Some(a) => js::max(js::min(least, natural), js::min(natural, a - around)),
    }
}

const BREAKPOINT: f64 = 640.0;

/// 横にも縦にも流せる図: 横に描き、ホストが許し、幅が境より狭く、横が収まらず、縦の方が狭ければ縦に描く。
pub fn turn_to_fit(
    options: &RenderOptions,
    mut draw: impl FnMut(bool) -> Result<Rendered>,
) -> Result<Rendered> {
    let across = draw(false)?;
    let Some(available) = available(options) else {
        return Ok(across);
    };
    if options.auto_direction == Some(false) || across.width <= available {
        return Ok(across);
    }
    let breakpoint = options
        .direction_breakpoint
        .filter(|&b| b > 0.0)
        .unwrap_or(BREAKPOINT);
    if available >= breakpoint {
        return Ok(across);
    }
    let down = draw(true)?;
    Ok(if down.width < across.width {
        down
    } else {
        across
    })
}

pub fn runs_across(direction: Option<&str>) -> bool {
    matches!(direction, Some("LR" | "RL"))
}

/// ゆとりを持って描き、ホストの幅に収まらず、詰めた方が狭ければ詰めて描く。
pub fn tighten(
    options: &RenderOptions,
    mut draw: impl FnMut(bool) -> Result<Rendered>,
) -> Result<Rendered> {
    let loose = draw(false)?;
    let Some(available) = available(options) else {
        return Ok(loose);
    };
    if loose.width <= available {
        return Ok(loose);
    }
    let tight = draw(true)?;
    Ok(if tight.width < loose.width {
        tight
    } else {
        loose
    })
}
