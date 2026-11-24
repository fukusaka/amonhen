/**
 * 画像の差分（History の詳細）。前と後を大きさを添えて並べるほか、スワイプ・オニオンスキン・違うピクセルで比べる
 * （アプリと同じ 4 つ）。足した・消した画像は片方だけを出す
 */
import type { ImageBlob } from '../history/protocol.ts';
import { el } from './api.ts';

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

/** 拡大縮小の倍率（0 は欄に合わせる）。欄をまたいで同じにする（開いている間だけ） */
let lastZoom = 0;

/** 画像の差分の欄。onShowText があれば（SVG）、帯に「文字で見る」を置く */
export function renderImageDiff(
  before: ImageBlob | undefined,
  after: ImageBlob | undefined,
  options: { onShowText?: () => void } = {},
): HTMLElement {
  const box = el('div', 'image-diff');
  if (!before && !after) {
    box.append(el('div', 'empty', 'Can’t read the image'));
    return box;
  }
  const both = before !== undefined && after !== undefined;
  const bar = el('div', 'image-modes');
  const modes = el('span', 'segmented');
  const stage = el('div', 'image-stage');
  // 帯: 比べ方（2 枚あるとき）・文字で見る（SVG）・拡大縮小（アプリと同じ）
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
  bar.append(
    spacer,
    zoomButton('−', 'Zoom Out', () => setZoom(currentZoom() / 1.25)),
    zoomLabel,
    zoomButton('+', 'Zoom In', () => setZoom(currentZoom() * 1.25)),
    zoomButton('Fit', 'Zoom to Fit', () => setZoom(0)),
  );
  zoomLabel.addEventListener('click', () => setZoom(lastZoom === 1 ? 0 : 1));
  box.append(bar, stage);
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
    const show = (mode: ImageMode) => {
      lastMode = mode;
      for (const b of modes.querySelectorAll<HTMLElement>('button')) b.classList.toggle('on', b.dataset.mode === mode);
      stage.replaceChildren(...render(mode));
      applyZoom();
    };
    const render = (mode: ImageMode): Node[] => {
      if (!both || mode === 'side') {
        const figures: Node[] = [];
        for (const [label, blob, img] of [
          ['Before', before, old],
          ['After', after, now],
        ] as const) {
          if (!blob || !img) continue;
          const figure = el('figure', 'image-figure');
          const frame = el('div', 'checker');
          frame.append(img.cloneNode());
          figure.append(frame, caption(both ? label : blob === after ? 'Added' : 'Deleted', blob, img));
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
      row.append(caption('Before', before!, old!), caption('After', after!, now!));
      return row;
    };
    for (const m of MODES) {
      const button = el('button', 'bar-button', m.label);
      button.dataset.mode = m.mode;
      button.title = m.tip;
      button.addEventListener('click', () => show(m.mode));
      if (both) modes.append(button);
    }
    show(both ? lastMode : 'side');
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
