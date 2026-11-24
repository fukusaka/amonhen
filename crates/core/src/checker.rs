//! 透けた絵の下に敷く市松模様の明暗（History の「すべてのファイル」と画像の比べ）。既定は絵の色から決める:
//! 見えている画素の明るさ（相対輝度を不透明度で重みづけした平均）が明るければ暗い市松模様、そうでなければ明るい
//! 市松模様。比べるときは両方の絵を合わせて 1 つに決める。

/// 明るい市松模様の 2 色（白と灰。sRGB の 0〜1）。
pub const LIGHT: (f64, f64) = (1.0, 0.86);
/// 暗い市松模様の 2 色。
pub const DARK: (f64, f64) = (0.16, 0.24);

/// 暗い市松模様にする明るさ（相対輝度）の境。2 つの市松模様の平均の相対輝度（明るい 0.858、暗い 0.034）に対する
/// コントラスト比が等しくなる明るさ: (0.858 + 0.05) / (Y + 0.05) = (Y + 0.05) / (0.034 + 0.05) から Y ≈ 0.226
/// （sRGB の灰色でおよそ 130/255）。これより明るい絵は暗い市松模様の方がよく見える。
pub const DARK_ABOVE: f64 = 0.226;

/// 1 つの絵で見る画素の数の上限（大きな絵は間引いて見る）。
const SAMPLES: usize = 1 << 18;

/// 画素の並び（どちらも不透明度を掛けた値）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Rgba,
    Bgra,
}

/// 絵の明るさを数えるもの。比べるときは 2 つの絵を `add` する。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tone {
    /// 相対輝度 × 不透明度の和。
    luminance: f64,
    /// 不透明度の和。
    alpha: f64,
    /// 透けた画素があった。
    transparent: bool,
}

impl Tone {
    /// 絵 1 枚の画素（`stride` バイトずつの行、4 バイトの画素、不透明度を掛けた値）を数える。
    pub fn add(&mut self, pixels: &[u8], width: usize, height: usize, stride: usize, order: Order) {
        if width == 0 || height == 0 || stride < width * 4 {
            return;
        }
        let step = ((width * height) as f64 / SAMPLES as f64)
            .sqrt()
            .ceil()
            .max(1.0) as usize;
        let (r, b) = match order {
            Order::Rgba => (0, 2),
            Order::Bgra => (2, 0),
        };
        for y in (0..height).step_by(step) {
            let Some(row) = pixels.get(y * stride..y * stride + width * 4) else {
                break;
            };
            for p in row.as_chunks::<4>().0.iter().step_by(step) {
                let a = p[3];
                if a < 255 {
                    self.transparent = true;
                }
                if a == 0 {
                    continue;
                }
                let alpha = f64::from(a) / 255.0;
                // 不透明度を掛けた値を戻してから、相対輝度に
                let channel = |v: u8| linear((f64::from(v) / 255.0 / alpha).min(1.0));
                let y = 0.2126 * channel(p[r]) + 0.7152 * channel(p[1]) + 0.0722 * channel(p[b]);
                self.luminance += y * alpha;
                self.alpha += alpha;
            }
        }
    }

    /// 暗い市松模様が既定か（見えている画素がなければ明るい方）。
    pub fn dark(&self) -> bool {
        self.alpha > 0.0 && self.luminance / self.alpha > DARK_ABOVE
    }

    /// 透けた画素があったか（なければ市松模様は見えない）。
    pub fn transparent(&self) -> bool {
        self.transparent
    }
}

/// sRGB の値（0〜1）を光の量に。
fn linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一色の絵（色 `rgb`、不透明度 `a`。不透明度を掛けた RGBA）。
    fn solid(rgb: u8, a: u8, n: usize) -> Vec<u8> {
        let v = (u32::from(rgb) * u32::from(a) / 255) as u8;
        [v, v, v, a].repeat(n * n)
    }

    fn tone(images: &[Vec<u8>], n: usize) -> Tone {
        let mut t = Tone::default();
        for i in images {
            t.add(i, n, n, n * 4, Order::Rgba);
        }
        t
    }

    #[test]
    fn picks_the_dark_checkerboard_for_light_pictures() {
        // 白いアイコン（半分透けた白も）は暗い市松模様、黒いアイコン・中間の灰は明るい市松模様
        assert!(tone(&[solid(255, 255, 8)], 8).dark());
        assert!(tone(&[solid(255, 128, 8)], 8).dark());
        assert!(!tone(&[solid(0, 255, 8)], 8).dark());
        assert!(!tone(&[solid(128, 255, 8)], 8).dark());
        assert!(tone(&[solid(140, 255, 8)], 8).dark());
        // 透けた所は数えない
        let mut icon = solid(255, 255, 8);
        icon.extend(solid(0, 0, 8).repeat(3));
        let mut t = Tone::default();
        t.add(&icon, 8, 32, 8 * 4, Order::Rgba);
        assert!(t.dark() && t.transparent());
        // 何もなければ明るい方
        assert!(!tone(&[solid(0, 0, 8)], 8).dark());
        assert!(!tone(&[], 8).dark());
    }

    #[test]
    fn decides_once_for_both_sides() {
        let t = tone(&[solid(255, 255, 8), solid(0, 255, 8)], 8);
        // 白と黒の平均の相対輝度は 0.5
        assert!(t.dark());
        assert!(!t.transparent());
        let t = tone(&[solid(0, 255, 8), solid(0, 255, 8), solid(255, 255, 8)], 8);
        assert!(t.dark());
        let t = tone(&[solid(0, 255, 8), solid(255, 0, 8)], 8);
        assert!(!t.dark() && t.transparent());
    }

    #[test]
    fn reads_bgra_and_samples_large_pictures() {
        let mut t = Tone::default();
        // 青だけ（相対輝度 0.07）と赤だけ（0.21）は明るい方、緑（0.72）は暗い方
        t.add(&[255, 0, 0, 255], 1, 1, 4, Order::Bgra);
        assert!(!t.dark());
        let mut t = Tone::default();
        t.add(&[0, 255, 0, 255], 1, 1, 4, Order::Bgra);
        assert!(t.dark());
        // 大きな絵は間引くが、決め方は同じ
        let n = 1024;
        let mut t = Tone::default();
        t.add(&solid(255, 255, n), n, n, n * 4, Order::Rgba);
        assert!(t.dark());
        // 行の終わりに余りのある stride と、短すぎるバッファでも止まらない
        let mut t = Tone::default();
        t.add(&[255; 10], 2, 2, 12, Order::Rgba);
        t.add(&[255; 3], 1, 1, 4, Order::Rgba);
        t.add(&[255; 16], 2, 2, 4, Order::Rgba);
    }
}
