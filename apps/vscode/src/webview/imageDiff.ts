/**
 * 画像の差分（History の詳細）。前と後を大きさを添えて並べるほか、スワイプ・オニオンスキン・違うピクセルで比べる
 * （アプリと同じ 4 つ）。足した・消した画像は片方だけを出す
 */
import type { ImageBlob } from '../history/protocol.ts';
import { el } from './api.ts';
import { CheckerTone } from './checkerTone.ts';

export type ImageMode = 'side' | 'swipe' | 'onion' | 'difference';

const MODES: { mode: ImageMode; label: string; tip: string }[] = [
  { mode: 'side', label: '2-Up', tip: 'Show the two versions side by side' },
  { mode: 'swipe', label: 'Swipe', tip: 'Drag to uncover the new version over the old one' },
  { mode: 'onion', label: 'Onion Skin', tip: 'Fade the new version over the old one' },
  { mode: 'difference', label: 'Difference', tip: 'Mark the pixels that differ' },
];

/** 見せ方は欄をまたいで同じにする（開いている間だけ） */
let lastMode: ImageMode = 'side';

/** 画像の拡張子か（SVG も絵として比べる） */
export function isImagePath(path: string): boolean {
  return /\.(png|jpe?g|gif|webp|bmp|ico|avif|svg)$/i.test(path);
}

function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 読んだ画像の大きさ（読めなければ 0） */
function load(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(img);
    img.src = url;
  });
}

/** 読み終えた画像（読めなくても） */
export function loaded(img: HTMLImageElement): Promise<HTMLImageElement> {
  if (img.complete) return Promise.resolve(img);
  return new Promise((resolve) => {
    img.addEventListener('load', () => resolve(img), { once: true });
    img.addEventListener('error', () => resolve(img), { once: true });
  });
}

/**
 * 市松模様の既定（絵の色から。比べるときは両方を合わせて 1 つ）と、透けた画素があるか。アプリと同じく、絵をそれぞれ
 * 256 × 256 に伸ばして並べた画素で数える
 */
export function checkerDefault(images: readonly HTMLImageElement[]): { dark: boolean; transparent: boolean } {
  const shown = images.filter((i) => i.naturalWidth > 0 && i.naturalHeight > 0);
  if (shown.length === 0) return { dark: false, transparent: false };
  const cell = 256;
  try {
    const canvas = document.createElement('canvas');
    canvas.width = cell * shown.length;
    canvas.height = cell;
    const g = canvas.getContext('2d', { willReadFrequently: true });
    if (!g) return { dark: false, transparent: false };
    shown.forEach((img, i) => g.drawImage(img, i * cell, 0, cell, cell));
    const tone = new CheckerTone();
    tone.add(g.getImageData(0, 0, canvas.width, cell).data, canvas.width, cell);
    return { dark: tone.dark(), transparent: tone.transparent() };
  } catch {
    return { dark: false, transparent: false };
  }
}

/** 欄ごとの今の切り替え */
const currentToggle = new WeakMap<HTMLElement, HTMLButtonElement>();

const CHECKER_ICON =
  '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="none" stroke="currentColor"/>' +
  '<path d="M2 3h4v3.5H2zM10 3h4v3.5h-4zM6 6.5h4V10H6zM2 10h4v3H2zM10 10h4v3h-4z" fill="currentColor"/></svg>';

/**
 * 市松模様の明暗の切り替え（アプリと同じ）。`scope` の中の市松模様を暗く・明るくする。既定は `images` の色から決め、
 * 透けた画素がなければ出さない。選んだ明暗は覚えない（ファイルを開き直すと既定に戻る）
 */
export function checkerToggle(scope: HTMLElement, images: readonly HTMLImageElement[]): HTMLButtonElement {
  const button = el('button', 'bar-button checker-toggle');
  currentToggle.set(scope, button);
  button.innerHTML = CHECKER_ICON;
  button.hidden = true;
  let dark = false;
  const apply = () => {
    // 置き換えられた切り替え（SVG の確認で部品を選び直した）は、もう触らない
    if (currentToggle.get(scope) !== button) return;
    scope.classList.toggle('checker-dark', dark);
    button.classList.toggle('on', dark);
    button.setAttribute('aria-pressed', String(dark));
    const title = dark ? 'Lighten the Checkerboard' : 'Darken the Checkerboard';
    button.title = title;
    button.setAttribute('aria-label', title);
  };
  button.addEventListener('click', () => {
    dark = !dark;
    apply();
  });
  apply();
  void Promise.all(images.map(loaded)).then((all) => {
    const tone = checkerDefault(all);
    dark = tone.dark;
    button.hidden = !tone.transparent;
    apply();
  });
  return button;
}

/** 拡大縮小の倍率（0 は欄に合わせる）。欄をまたいで同じにする（開いている間だけ） */
let lastZoom = 0;

/** 画像の比較の欄の作り方 */
export interface ImageDiffOptions {
  /** あれば（SVG）、帯に「文字で見る」を置く */
  onShowText?: () => void;
  /** 前と後の見出し（なければ Before・After、片方だけなら Added・Deleted） */
  titles?: [string, string];
  /** 絵の下の説明（1 行。SVG のモバイルの確認の、絵の作り方） */
  caption?: string;
  /** 帯の左に置く部品（SVG のモバイルの確認の、部品の切り替え） */
  accessory?: HTMLElement;
  /** 絵の下に置く説明（SVG のモバイルの確認の、崩れる機能の一覧） */
  notes?: HTMLElement;
  /** 最初の比べ方（あれば、選んだ比べ方をほかの欄に持ち越さない。SVG のモバイルの確認はいつも並べるから。アプリと同じ） */
  mode?: ImageMode;
}

/** 画像の差分の欄（SVG のモバイルの確認の、元の絵と部品での見え方の比較にも使う） */
export function renderImageDiff(before: ImageBlob | undefined, after: ImageBlob | undefined, options: ImageDiffOptions = {}): HTMLElement {
  const box = el('div', 'image-diff');
  if (!before && !after) {
    box.append(el('div', 'empty', 'Can’t read the image'));
    return box;
  }
  const both = before !== undefined && after !== undefined;
  const bar = el('div', 'image-modes');
  const modes = el('span', 'segmented');
  const stage = el('div', 'image-stage');
  // 帯: 置く部品・比べ方（2 枚あるとき）・文字で見る（SVG）・拡大縮小（アプリと同じ）
  if (options.accessory) bar.append(options.accessory);
  bar.append(modes);
  if (options.onShowText) {
    const text = el('button', 'bar-button', 'Show as Text');
    text.title = 'Show the SVG’s text and its diff';
    text.addEventListener('click', () => options.onShowText?.());
    bar.append(text);
  }
  const zoomLabel = el('button', 'bar-button zoom-label');
  zoomLabel.title = 'Actual Size (100%) or Zoom to Fit';
  const zoomButton = (text: string, tip: string, run: () => void) => {
    const b = el('button', 'bar-button', text);
    b.title = tip;
    b.addEventListener('click', run);
    return b;
  };
  const spacer = el('span', 'spacer');
  // 市松模様の明暗（絵を読んでから置く。アプリと同じく拡大縮小の左）
  const checkerSlot = el('span', 'checker-slot');
  bar.append(
    spacer,
    checkerSlot,
    zoomButton('−', 'Zoom Out', () => setZoom(currentZoom() / 1.25)),
    zoomLabel,
    zoomButton('+', 'Zoom In', () => setZoom(currentZoom() * 1.25)),
    zoomButton('Fit', 'Zoom to Fit', () => setZoom(0)),
  );
  zoomLabel.addEventListener('click', () => setZoom(lastZoom === 1 ? 0 : 1));
  box.append(bar, stage);
  if (options.caption) box.append(el('div', 'image-caption image-note', options.caption));
  if (options.notes) box.append(options.notes);
  const [beforeTitle, afterTitle] = options.titles ?? ['Before', 'After'];
  /** 今の倍率（合わせているときは、いちばん大きい画像の見えている倍率） */
  const currentZoom = () => {
    if (lastZoom > 0) return lastZoom;
    const img = stage.querySelector<HTMLImageElement | HTMLCanvasElement>('img, canvas');
    const natural = img instanceof HTMLImageElement ? img.naturalWidth : (img?.width ?? 0);
    return img && natural ? img.clientWidth / natural : 1;
  };
  const setZoom = (zoom: number) => {
    lastZoom = zoom === 0 ? 0 : Math.min(32, Math.max(0.05, zoom));
    applyZoom();
  };
  /** 画像と重ねた枠の幅を倍率に合わせる（合わせるなら欄の幅まで） */
  const applyZoom = () => {
    zoomLabel.textContent = lastZoom === 0 ? 'Fit' : `${Math.round(lastZoom * 100)}%`;
    for (const img of stage.querySelectorAll<HTMLImageElement>('.image-side img')) {
      img.style.maxWidth = lastZoom === 0 ? '100%' : 'none';
      img.style.width = lastZoom === 0 ? '' : `${Math.round(img.naturalWidth * lastZoom)}px`;
    }
    const overlay = stage.querySelector<HTMLElement>('.image-overlay');
    if (overlay) {
      const width = Number(overlay.dataset.width ?? 0);
      overlay.style.maxWidth = lastZoom === 0 ? `${width}px` : 'none';
      overlay.style.width = lastZoom === 0 ? '' : `${Math.round(width * lastZoom)}px`;
    }
  };
  stage.addEventListener(
    'wheel',
    (e) => {
      if (!e.metaKey && !e.ctrlKey) return;
      e.preventDefault();
      setZoom(currentZoom() * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
    },
    { passive: false },
  );

  const caption = (label: string, blob: ImageBlob, img: HTMLImageElement) =>
    el('div', 'image-caption', `${label} · ${img.naturalWidth} × ${img.naturalHeight} · ${sizeText(blob.bytes)}`);

  void Promise.all([before ? load(before.url) : undefined, after ? load(after.url) : undefined]).then(([old, now]) => {
    checkerSlot.replaceWith(checkerToggle(box, [old, now].filter((i): i is HTMLImageElement => i !== undefined)));
    const show = (mode: ImageMode) => {
      if (!options.mode) lastMode = mode;
      for (const b of modes.querySelectorAll<HTMLElement>('button')) b.classList.toggle('on', b.dataset.mode === mode);
      stage.replaceChildren(...render(mode));
      applyZoom();
    };
    const render = (mode: ImageMode): Node[] => {
      if (!both || mode === 'side') {
        const figures: Node[] = [];
        for (const [label, blob, img] of [
          [beforeTitle, before, old],
          [afterTitle, after, now],
        ] as const) {
          if (!blob || !img) continue;
          const figure = el('figure', 'image-figure');
          const frame = el('div', 'checker');
          frame.append(img.cloneNode());
          const single = blob === after ? 'Added' : 'Deleted';
          figure.append(frame, caption(both || options.titles ? label : single, blob, img));
          figures.push(figure);
        }
        return [wrap('image-side', figures)];
      }
      const width = Math.max(old!.naturalWidth, now!.naturalWidth);
      const height = Math.max(old!.naturalHeight, now!.naturalHeight);
      const frame = el('div', 'checker image-overlay');
      frame.dataset.width = String(width);
      frame.style.aspectRatio = `${width} / ${height || 1}`;
      frame.style.maxWidth = `${width}px`;
      const under = old!.cloneNode() as HTMLImageElement;
      const over = now!.cloneNode() as HTMLImageElement;
      const slider = el('input', 'image-slider');
      slider.type = 'range';
      slider.min = '0';
      slider.max = '100';
      if (mode === 'swipe') {
        slider.value = '50';
        const apply = () => (over.style.clipPath = `inset(0 0 0 ${slider.value}%)`);
        slider.addEventListener('input', apply);
        apply();
        frame.append(under, over);
        return [frame, slider, captions()];
      }
      if (mode === 'onion') {
        slider.value = '50';
        const apply = () => (over.style.opacity = `${Number(slider.value) / 100}`);
        slider.addEventListener('input', apply);
        apply();
        frame.append(under, over);
        return [frame, slider, captions()];
      }
      const canvas = el('canvas');
      canvas.width = width;
      canvas.height = height;
      const count = el('div', 'image-caption');
      frame.append(canvas);
      paintDifference(canvas, old!, now!, count);
      return [frame, count];
    };
    const captions = () => {
      const row = el('div', 'image-captions');
      row.append(caption(beforeTitle, before!, old!), caption(afterTitle, after!, now!));
      return row;
    };
    for (const m of MODES) {
      const button = el('button', 'bar-button', m.label);
      button.dataset.mode = m.mode;
      button.title = m.tip;
      button.addEventListener('click', () => show(m.mode));
      if (both) modes.append(button);
    }
    show(both ? (options.mode ?? lastMode) : 'side');
  });
  return box;
}

function wrap(className: string, children: Node[]): HTMLElement {
  const div = el('div', className);
  div.append(...children);
  return div;
}

/** 違うピクセルを赤で、同じところは薄く描く。数を `count` に書く */
function paintDifference(canvas: HTMLCanvasElement, old: HTMLImageElement, now: HTMLImageElement, count: HTMLElement): void {
  const { width, height } = canvas;
  const read = (img: HTMLImageElement) => {
    const c = document.createElement('canvas');
    c.width = width;
    c.height = height;
    const g = c.getContext('2d')!;
    g.drawImage(img, 0, 0);
    return g.getImageData(0, 0, width, height).data;
  };
  const a = read(old);
  const b = read(now);
  const g = canvas.getContext('2d')!;
  const out = g.createImageData(width, height);
  let differ = 0;
  for (let i = 0; i < a.length; i += 4) {
    const same = a[i] === b[i] && a[i + 1] === b[i + 1] && a[i + 2] === b[i + 2] && a[i + 3] === b[i + 3];
    if (same) {
      // 同じところは新しい方を薄い灰色で
      const gray = (b[i]! + b[i + 1]! + b[i + 2]!) / 3;
      out.data[i] = out.data[i + 1] = out.data[i + 2] = gray;
      out.data[i + 3] = Math.round(b[i + 3]! * 0.25);
    } else {
      differ++;
      out.data[i] = 255;
      out.data[i + 1] = 40;
      out.data[i + 2] = 40;
      out.data[i + 3] = 255;
    }
  }
  g.putImageData(out, 0, 0);
  const total = width * height;
  // アプリと同じく、全体のうちの数と割合
  count.textContent =
    differ === 0 ? 'No pixels differ' : `${differ.toLocaleString()} of ${total.toLocaleString()} pixels differ (${((differ * 100) / total).toFixed(1)}%)`;
}
