/**
 * partial clone で中身が手元にないときの帯:「N 個のファイルの中身がまだ手元にありません［Fetch Contents］」（アプリの
 * MissingContentsBand と同じ）。読む画面は勝手に取りに行かず、押したときだけ `fetch` で拡張ホストに頼む。取れたら
 * 持ち主が出し直し、取れなければ `failed` でわけを出してもう一度押せるようにする。次のコミットではまた聞く
 */
import { el } from './api.ts';

export interface MissingBand {
  readonly root: HTMLElement;
  /** 取れなかった（ボタンをまた押せるようにし、わけを出す。もとの文はツールチップに） */
  failed(error: string): void;
}

/** 差分の帯の文: `files` 個のファイルの中身がまだ手元にない */
export function missingFilesText(files: number): string {
  return `The contents of ${files} file${files === 1 ? '' : 's'} aren’t here yet (partial clone).`;
}

export function missingBand(message: string, fetch: () => void): MissingBand {
  const root = el('div', 'missing-band');
  root.setAttribute('role', 'group');
  root.setAttribute('aria-label', message);
  // partial clone の共通のアイコン（サイドバー・アプリと同じ形）
  const icon = el('span', 'shared-icon partial');
  icon.setAttribute('aria-hidden', 'true');
  const label = el('span', 'missing-text', message);
  const button = el('button', 'secondary', 'Fetch Contents');
  button.title = 'Fetch just these contents from the remote, in one go, and show them.';
  button.addEventListener('click', () => {
    button.disabled = true;
    label.textContent = 'Fetching the contents…';
    label.title = '';
    fetch();
  });
  root.append(icon, label, el('span', 'missing-spacer'), button);
  return {
    root,
    failed(error) {
      button.disabled = false;
      label.textContent = `Couldn’t fetch the contents: ${error}`;
      label.title = message;
    },
  };
}
