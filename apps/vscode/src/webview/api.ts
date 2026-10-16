/** Webview の共通部分。acquireVsCodeApi は 1 回しか呼べないので、ここでまとめて持つ */
import type { HostMessage, ViewName, WebviewMessage } from '../view/protocol.ts';

export interface State {
  view?: ViewName;
  /** History の詳細の幅 */
  detailsWidth?: number;
}

declare function acquireVsCodeApi(): {
  postMessage(message: WebviewMessage): void;
  getState(): State | undefined;
  setState(state: State): void;
};

const api = acquireVsCodeApi();

export function post(message: WebviewMessage): void {
  api.postMessage(message);
}

export function getState(): State {
  return api.getState() ?? {};
}

export function updateState(patch: Partial<State>): void {
  api.setState({ ...getState(), ...patch });
}

/** 右側に出す画面 */
export interface View {
  readonly root: HTMLElement;
  onMessage(message: HostMessage): void;
  /** 表示されたとき。隠れている間は大きさが 0 なので、描き直す */
  onShow(): void;
}

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

export function formatDate(seconds: number): string {
  const d = new Date(seconds * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
