/** 署名の印の形と色（アプリの SignatureStatus の SF Symbols と色に近い文字と、テーマの色） */
import type { SignatureView } from '../history/protocol.ts';
import { el } from './api.ts';

const GLYPHS: Record<SignatureView['kind'], { glyph: string; cls: string }> = {
  verified: { glyph: '✔', cls: 'sig-good' },
  untrusted: { glyph: '✔', cls: 'sig-warn' },
  signerMismatch: { glyph: '?', cls: 'sig-warn' },
  expired: { glyph: '⏲', cls: 'sig-old' },
  revoked: { glyph: '✖', cls: 'sig-bad' },
  bad: { glyph: '✖', cls: 'sig-bad' },
  noKey: { glyph: '?', cls: 'sig-none' },
  unsupported: { glyph: '!', cls: 'sig-none' },
  pending: { glyph: '…', cls: 'sig-none' },
};

/** 印（ツールチップに言葉と説明）。信用度が弱い verified は縁だけ（薄く） */
export function signatureMark(view: SignatureView): HTMLElement {
  const { glyph, cls } = GLYPHS[view.kind];
  const mark = el('span', `sig ${cls}${view.kind === 'verified' && (view.trust ?? 3) < 3 ? ' weak' : ''}`, glyph);
  mark.title = `${view.label} — ${view.explanation}${view.signer ? `\nSigned by ${view.signer}` : ''}`;
  return mark;
}
