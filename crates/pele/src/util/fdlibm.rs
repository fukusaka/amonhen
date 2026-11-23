//! V8 の `Math.sin`・`Math.cos`・`Math.atan`・`Math.atan2`・`Math.acos`（V8 の `base/ieee754.cc`、fdlibm から）。
//! V8 の答えは正しく丸めたものとは限らず、OS の libm と最後の桁で違うことがある。それが配置を何度も繰り返す図
//! （venn の円の緩和など）では目に見える違いに育つので、同じ手順で計算する。
//!
//! 64 ビットの ARM の V8 は clang が `a * b + c` の形を 1 つの式の中で FMA にまとめて作られている
//! （`-ffp-contract=on`）。pele の例（`crates/core/tests/data/pele`）はそこで描いたので、同じ所を `mul_add` にした。
//! まとめ方は clang と同じ: 足し算・引き算の左の項が掛け算ならそれを、でなければ右の項の掛け算をまとめる。
//! 変数に置いた掛け算はまとめない。node の答えと 20 万の値で同じことを確かめた。
//!
//! fdlibm の告知:
//!
//! ```text
//! Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//!
//! Developed at SunSoft, a Sun Microsystems, Inc. business.
//! Permission to use, copy, modify, and distribute this
//! software is freely granted, provided that this notice
//! is preserved.
//! ```

// fdlibm の定数はそのままの桁で書く。
#![allow(clippy::excessive_precision, clippy::approx_constant)]

fn high(x: f64) -> i32 {
    (x.to_bits() >> 32) as u32 as i32
}

fn low(x: f64) -> u32 {
    x.to_bits() as u32
}

fn with_low(x: f64, lo: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0xFFFF_FFFF_0000_0000) | lo as u64)
}

fn words(hi: u32, lo: u32) -> f64 {
    f64::from_bits(((hi as u64) << 32) | lo as u64)
}

const INVPIO2: f64 = 6.36619772367581382433e-01;
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;

/// `__ieee754_rem_pio2`: x を π/2 の n 倍と余り（y0 + y1）に分ける。|x| が 2^19·π/2 を超えるときは
/// fdlibm の長い経路の代わりに OS の関数で求める（図の数ではまず届かない）。
fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let hx = high(x);
    let ix = hx & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return (0, x, 0.0);
    }
    if ix < 0x4002_D97C {
        // |x| < 3π/4
        if hx > 0 {
            let mut z = x - PIO2_1;
            if ix != 0x3FF9_21FB {
                let y0 = z - PIO2_1T;
                return (1, y0, (z - y0) - PIO2_1T);
            }
            z -= PIO2_2;
            let y0 = z - PIO2_2T;
            return (1, y0, (z - y0) - PIO2_2T);
        }
        let mut z = x + PIO2_1;
        if ix != 0x3FF9_21FB {
            let y0 = z + PIO2_1T;
            return (-1, y0, (z - y0) + PIO2_1T);
        }
        z += PIO2_2;
        let y0 = z + PIO2_2T;
        return (-1, y0, (z - y0) + PIO2_2T);
    }
    if ix <= 0x4139_21FB {
        let t = x.abs();
        let n = t.mul_add(INVPIO2, 0.5) as i32;
        let fn_ = n as f64;
        let mut r = (-fn_).mul_add(PIO2_1, t);
        let mut w = fn_ * PIO2_1T;
        // fdlibm は桁落ちのない n を表で見分けて近道をするが、そのときは下の確かめでも同じ答えになる。
        let j = ix >> 20;
        let mut y0 = r - w;
        let i = j - ((high(y0) >> 20) & 0x7FF);
        if i > 16 {
            let t = r;
            w = fn_ * PIO2_2;
            r = t - w;
            w = fn_.mul_add(PIO2_2T, -((t - r) - w));
            y0 = r - w;
            let i = j - ((high(y0) >> 20) & 0x7FF);
            if i > 49 {
                let t = r;
                w = fn_ * PIO2_3;
                r = t - w;
                w = fn_.mul_add(PIO2_3T, -((t - r) - w));
                y0 = r - w;
            }
        }
        let y1 = (r - y0) - w;
        return if hx < 0 { (-n, -y0, -y1) } else { (n, y0, y1) };
    }
    // 大きな数: 近い値で済ませる。
    let n = (x / std::f64::consts::FRAC_PI_2).round();
    let y0 = x - n * std::f64::consts::FRAC_PI_2;
    ((n as i64 & 3) as i32, y0, 0.0)
}

const S1: f64 = -1.66666666666666324348e-01;
const S2: f64 = 8.33333333332248946124e-03;
const S3: f64 = -1.98412698298579493134e-04;
const S4: f64 = 2.75573137070700676789e-06;
const S5: f64 = -2.50507602534068634195e-08;
const S6: f64 = 1.58969099521155010221e-10;

fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    let ix = high(x) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 && x as i32 == 0 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = z.mul_add(z.mul_add(z.mul_add(z.mul_add(S6, S5), S4), S3), S2);
    if iy == 0 {
        v.mul_add(z.mul_add(r, S1), x)
    } else {
        x - (-v).mul_add(S1, z.mul_add(0.5f64.mul_add(y, -(v * r)), -y))
    }
}

const C1: f64 = 4.16666666666666019037e-02;
const C2: f64 = -1.38888888888741095749e-03;
const C3: f64 = 2.48015872894767294178e-05;
const C4: f64 = -2.75573143513906633035e-07;
const C5: f64 = 2.08757232129817482790e-09;
const C6: f64 = -1.13596475577881948265e-11;

fn kernel_cos(x: f64, y: f64) -> f64 {
    let ix = high(x) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 && x as i32 == 0 {
        return 1.0;
    }
    let z = x * x;
    let r = z * z.mul_add(
        z.mul_add(z.mul_add(z.mul_add(z.mul_add(C6, C5), C4), C3), C2),
        C1,
    );
    if ix < 0x3FD3_3333 {
        return 1.0 - 0.5f64.mul_add(z, -z.mul_add(r, -(x * y)));
    }
    let qx = if ix > 0x3FE9_0000 {
        0.28125
    } else {
        words((ix - 0x0020_0000) as u32, 0)
    };
    let iz = 0.5f64.mul_add(z, -qx);
    let a = 1.0 - qx;
    a - (iz - z.mul_add(r, -(x * y)))
}

/// `Math.sin`。
pub fn sin(x: f64) -> f64 {
    let ix = high(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return kernel_sin(x, 0.0, 0);
    }
    if ix >= 0x7FF0_0000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_sin(y0, y1, 1),
        1 => kernel_cos(y0, y1),
        2 => -kernel_sin(y0, y1, 1),
        _ => -kernel_cos(y0, y1),
    }
}

/// `Math.cos`。
pub fn cos(x: f64) -> f64 {
    let ix = high(x) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        return kernel_cos(x, 0.0);
    }
    if ix >= 0x7FF0_0000 {
        return f64::NAN;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => kernel_cos(y0, y1),
        1 => -kernel_sin(y0, y1, 1),
        2 => -kernel_cos(y0, y1),
        _ => kernel_sin(y0, y1, 1),
    }
}

const ATANHI: [f64; 4] = [
    4.63647609000806093515e-01,
    7.85398163397448278999e-01,
    9.82793723247329054082e-01,
    1.57079632679489655800e+00,
];
const ATANLO: [f64; 4] = [
    2.26987774529616870924e-17,
    3.06161699786838301793e-17,
    1.39033110312309984516e-17,
    6.12323399573676603587e-17,
];
const AT: [f64; 11] = [
    3.33333333333329318027e-01,
    -1.99999999998764832476e-01,
    1.42857142725034663711e-01,
    -1.11111104054623557880e-01,
    9.09088713343650656196e-02,
    -7.69187620504482999495e-02,
    6.66107313738753120669e-02,
    -5.83357013379057348645e-02,
    4.97687799461593236017e-02,
    -3.65315727442169155270e-02,
    1.62858201153657823623e-02,
];

/// `Math.atan`。
pub fn atan(x: f64) -> f64 {
    let hx = high(x);
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x4410_0000 {
        if x.is_nan() {
            return x + x;
        }
        return if hx > 0 {
            ATANHI[3] + ATANLO[3]
        } else {
            -ATANHI[3] - ATANLO[3]
        };
    }
    let mut x = x;
    let id: i32;
    if ix < 0x3FDC_0000 {
        if ix < 0x3E40_0000 {
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3FF3_0000 {
            if ix < 0x3FE6_0000 {
                id = 0;
                x = 2.0f64.mul_add(x, -1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            id = 2;
            x = (x - 1.5) / 1.5f64.mul_add(x, 1.0);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * w.mul_add(
        w.mul_add(
            w.mul_add(w.mul_add(w.mul_add(AT[10], AT[8]), AT[6]), AT[4]),
            AT[2],
        ),
        AT[0],
    );
    let s2 = w * w.mul_add(
        w.mul_add(w.mul_add(w.mul_add(AT[9], AT[7]), AT[5]), AT[3]),
        AT[1],
    );
    if id < 0 {
        return (-x).mul_add(s1 + s2, x);
    }
    let id = id as usize;
    let z = ATANHI[id] - (x.mul_add(s1 + s2, -ATANLO[id]) - x);
    if hx < 0 { -z } else { z }
}

const PI: f64 = 3.1415926535897931160E+00;
const PI_O_4: f64 = 7.8539816339744827900E-01;
const PI_O_2: f64 = 1.5707963267948965580E+00;
const PI_LO: f64 = 1.2246467991473531772E-16;
const TINY: f64 = 1.0e-300;

/// `Math.atan2(y, x)`。
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let (hx, lx) = (high(x), low(x));
    let (hy, ly) = (high(y), low(y));
    let ix = hx & 0x7FFF_FFFF;
    let iy = hy & 0x7FFF_FFFF;
    if (hx.wrapping_sub(0x3FF0_0000) as u32 | lx) == 0 {
        return atan(y);
    }
    let mut m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if (iy as u32 | ly) == 0 {
        return match m {
            0 | 1 => y,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if (ix as u32 | lx) == 0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    if ix == 0x7FF0_0000 {
        if iy == 0x7FF0_0000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if iy == 0x7FF0_0000 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        m &= 1;
        0.5f64.mul_add(PI_LO, PI_O_2)
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

const PIO2_HI: f64 = 1.57079632679489655800e+00;
const PIO2_LO: f64 = 6.12323399573676603587e-17;
const PS0: f64 = 1.66666666666666657415e-01;
const PS1: f64 = -3.25565818622400915405e-01;
const PS2: f64 = 2.01212532134862925881e-01;
const PS3: f64 = -4.00555345006794114027e-02;
const PS4: f64 = 7.91534994289814532176e-04;
const PS5: f64 = 3.47933107596021167570e-05;
const QS1: f64 = -2.40339491173441421878e+00;
const QS2: f64 = 2.02094576023350569471e+00;
const QS3: f64 = -6.88283971605453293030e-01;
const QS4: f64 = 7.70381505559019352791e-02;

/// `Math.acos`。
pub fn acos(x: f64) -> f64 {
    let hx = high(x);
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x3FF0_0000 {
        if ((ix - 0x3FF0_0000) as u32 | low(x)) == 0 {
            return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
        }
        return f64::NAN;
    }
    let pq = |z: f64| {
        let p = z * z.mul_add(
            z.mul_add(z.mul_add(z.mul_add(z.mul_add(PS5, PS4), PS3), PS2), PS1),
            PS0,
        );
        let q = z.mul_add(z.mul_add(z.mul_add(z.mul_add(QS4, QS3), QS2), QS1), 1.0);
        (p, q)
    };
    if ix < 0x3FE0_0000 {
        if ix <= 0x3C60_0000 {
            return PIO2_HI + PIO2_LO;
        }
        let z = x * x;
        let (p, q) = pq(z);
        let r = p / q;
        return PIO2_HI - (x - (-x).mul_add(r, PIO2_LO));
    }
    if hx < 0 {
        let z = (1.0 + x) * 0.5;
        let (p, q) = pq(z);
        let s = z.sqrt();
        let r = p / q;
        let w = r.mul_add(s, -PIO2_LO);
        return (-2.0f64).mul_add(s + w, PI);
    }
    let z = (1.0 - x) * 0.5;
    let s = z.sqrt();
    let df = with_low(s, 0);
    let c = (-df).mul_add(df, z) / (s + df);
    let (p, q) = pq(z);
    let r = p / q;
    let w = r.mul_add(s, c);
    2.0 * (df + w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_v8() {
        // node（64 ビットの ARM）の答え。OS の libm とは最後の桁が違うものを含む。
        assert_eq!(cos(6.0), 0.9601702866503661);
        assert_eq!(cos(131.0), 0.5842088171092893);
        assert_eq!(cos(-1.1519173063162575), 0.40673664307580015);
        assert_eq!(sin(0.5), 0.479425538604203);
        assert_eq!(sin(57.0), 0.43616475524782494);
        assert_eq!(atan2(1.0, 3.0), 0.3217505543966422);
        assert_eq!(atan2(-2.0, -0.3), -1.719686274404394);
        assert_eq!(acos(0.7), 0.7953988301841436);
        assert_eq!(acos(-0.9), 2.6905658417935308);
        assert_eq!(acos(0.2), 1.369438406004566);
    }
}
