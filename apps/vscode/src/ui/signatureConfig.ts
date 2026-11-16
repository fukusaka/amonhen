/**
 * 署名の検証の設定（`gpg.ssh.allowedSignersFile`・`gpg.minTrustLevel` など `gpg.*`）を書いた知らせ。History と
 * タグの印は覚えていた検証の結果を捨てて確かめ直す（アプリの `signatureCheckChanged`）
 */
import * as vscode from 'vscode';

const emitter = new vscode.EventEmitter<void>();

export const onDidChangeSignatureConfig = emitter.event;

/** 書いた設定のキーが検証に関わるなら知らせる */
export function signatureConfigWritten(keys: readonly string[]): void {
  if (keys.some((k) => k.toLowerCase().startsWith('gpg.'))) emitter.fire();
}
