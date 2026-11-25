/**
 * 透けた絵の下に敷く市松模様の明暗（コアの `checker::Tone` と同じ決め方）。見えている画素の明るさ（相対輝度を
 * 不透明度で重みづけした平均）が明るければ暗い市松模様、そうでなければ明るい市松模様。比べるときは両方の絵を
 * 合わせて 1 つに決める。DOM に触れない（試験から使う）
 */

/**
 * 暗い市松模様にする明るさ（相対輝度）の境。明るい市松模様（白と 0.86 の灰、平均 0.858）と暗い市松模様（0.16 と
 * 0.24、平均 0.034）に対するコントラスト比が等しくなる明るさ
 */
export const DARK_ABOVE = 0.226;

/** 1 つの絵で見る画素の数の上限（大きな絵は間引いて見る） */
const SAMPLES = 1 << 18;

/** 絵の明るさを数えるもの。比べるときは 2 つの絵を `add` する */
export class CheckerTone {
  /** 相対輝度 × 不透明度の和 */
  private luminance = 0;
  /** 不透明度の和 */
  private alpha = 0;
  /** 透けた画素があった */
  private seenTransparent = false;

  /**
   * 絵 1 枚の画素を数える。`pixels` は `width` × `height` の RGBA（canvas の `ImageData` と同じく、不透明度を掛けて
   * いない値）
   */
  add(pixels: ArrayLike<number>, width: number, height: number): void {
    if (width <= 0 || height <= 0 || pixels.length < width * height * 4) return;
    const step = Math.max(1, Math.ceil(Math.sqrt((width * height) / SAMPLES)));
    for (let y = 0; y < height; y += step) {
      for (let x = 0; x < width; x += step) {
        const i = (y * width + x) * 4;
        const a = pixels[i + 3]!;
        if (a < 255) this.seenTransparent = true;
        if (a === 0) continue;
        const alpha = a / 255;
        const lum = 0.2126 * linear(pixels[i]! / 255) + 0.7152 * linear(pixels[i + 1]! / 255) + 0.0722 * linear(pixels[i + 2]! / 255);
        this.luminance += lum * alpha;
        this.alpha += alpha;
      }
    }
  }

  /** 暗い市松模様が既定か（見えている画素がなければ明るい方） */
  dark(): boolean {
    return this.alpha > 0 && this.luminance / this.alpha > DARK_ABOVE;
  }

  /** 透けた画素があったか（なければ市松模様は見えない） */
  transparent(): boolean {
    return this.seenTransparent;
  }
}

/** sRGB の値（0〜1）を光の量に */
function linear(v: number): number {
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}
