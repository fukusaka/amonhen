/**
 * git と ssh が資格情報を尋ねる先（askpass。アプリの `amonhen-askpass` と同じ役）。拡張は端末なしで git を動かす
 * ので、尋ねられると失敗していた。起動のときに、この拡張ホストに問いを送る小さなスクリプトを作る（渡し方は
 * `git/askpass.ts`）。問いは VSCode の入力欄で尋ね、答えをスクリプトの標準出力に返す。キャンセルなら 1 で
 * 終わる（git は認証に失敗したことにする）
 *
 * スクリプトは VSCode の Node（`ELECTRON_RUN_AS_NODE`）で、作ったフォルダの `askpass.js` を動かす。問いの窓口は
 * そのフォルダの Unix ドメインソケット（Windows は名前付きパイプ）。フォルダは本人だけが読み書きできる
 */
import * as vscode from 'vscode';
import { Askpass, hiddenAnswer } from '../git/askpass.ts';

/** 窓口を開き、問いを VSCode の入力欄で尋ねる */
export function startAskpass(): Promise<Askpass> {
  return Askpass.start(askInVSCode);
}

/** VSCode の入力欄で尋ねる。ほかを触っても閉じない（git が答えを待っている） */
async function askInVSCode(prompt: string): Promise<string | undefined> {
  return vscode.window.showInputBox({
    title: 'Amon Hen',
    prompt: prompt.trim(),
    password: hiddenAnswer(prompt),
    ignoreFocusOut: true,
  });
}
