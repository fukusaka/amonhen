/**
 * コミットとタグの署名の見せ方（アプリの `SignatureStatus` と同じ決まり）。コアの検証の結果（`%G?` の 1 文字と
 * 信用度、署名した鍵のメールアドレス）に、署名者がコミッター（タグはタガー）と一致するかを加えて決める。
 * vscode に依存しない
 */

/** コアが返す検証の結果（`signature_json`） */
export interface SignatureCheck {
  status: string;
  signer?: string | null;
  key?: string | null;
  fingerprint?: string | null;
  trust?: string | null;
  emails?: { email: string; validity: string }[];
}

export type SignatureKind =
  | 'verified'
  | 'untrusted'
  | 'signerMismatch'
  | 'expired'
  | 'revoked'
  | 'bad'
  | 'noKey'
  | 'unsupported'
  | 'pending';

export interface SignatureStatus {
  kind: SignatureKind;
  /** 信用度の段階（3 は ultimate・full、2 は marginal）。verified のときだけ */
  trust?: number;
}

/**
 * 検証の結果から決める。`check` がなければ（署名はあるがまだ検証していない）pending。署名者がコミッターと違うと
 * するのは、鍵のメールアドレスを読めて、どれも一致しないときだけ
 */
export function signatureStatus(
  check: SignatureCheck | undefined | null,
  committerEmail = '',
  kind?: string | null,
): SignatureStatus {
  if (!check) return { kind: 'pending' };
  switch (check.status) {
    case 'G':
    case 'U': {
      const level = trustLevel(check.trust ?? '', kind === 'ssh');
      if (check.status === 'U' || level < 2) return { kind: 'untrusted' };
      if (committerMatch(check.emails ?? [], committerEmail) === 'mismatched') return { kind: 'signerMismatch' };
      return { kind: 'verified', trust: level };
    }
    case 'X':
    case 'Y':
      return { kind: 'expired' };
    case 'R':
      return { kind: 'revoked' };
    case 'B':
      return { kind: 'bad' };
    case 'V':
      return { kind: 'unsupported' };
    default:
      return { kind: 'noKey' };
  }
}

/**
 * 信用度の段階（3 は ultimate・full、2 は marginal、1 は undefined、0 は never）。SSH の署名は allowed signers に
 * 載っていれば正しい（G）になるので、信用しているものとして 3 にする
 */
export function trustLevel(trust: string, ssh: boolean): number {
  if (ssh) return 3;
  switch (trust.toLowerCase()) {
    case 'ultimate':
    case 'full':
    case 'fully':
      return 3;
    case 'marginal':
      return 2;
    case 'never':
      return 0;
    default:
      return 1;
  }
}

/**
 * 署名した鍵のメールアドレスと、コミッターのメールアドレスの関係。有効なユーザー ID（u・f・m）と一致すれば
 * matched、有効性の分からないものとだけ一致すれば unconfirmed、どれとも一致しなければ mismatched（失効・期限切れの
 * ユーザー ID との一致は数えない）、読めなければ unknown
 */
export function committerMatch(
  emails: readonly { email: string; validity: string }[],
  committerEmail: string,
): 'matched' | 'unconfirmed' | 'mismatched' | 'unknown' {
  const email = committerEmail.toLowerCase();
  if (emails.length === 0 || !email) return 'unknown';
  const same = emails.filter((e) => e.email.toLowerCase() === email);
  if (same.some((e) => ['u', 'f', 'm'].includes(e.validity))) return 'matched';
  if (same.some((e) => !['r', 'e', 'n'].includes(e.validity))) return 'unconfirmed';
  return 'mismatched';
}

/** 短い言葉（アプリと同じく GitHub に合わせる） */
export function signatureLabel(status: SignatureStatus): string {
  switch (status.kind) {
    case 'verified':
      return 'Verified';
    case 'untrusted':
      return 'Untrusted key';
    case 'signerMismatch':
      return 'Unverified signer';
    case 'expired':
      return 'Expired';
    case 'revoked':
      return 'Revoked key';
    case 'bad':
      return 'Bad signature';
    case 'noKey':
      return 'No key';
    case 'unsupported':
      return 'Can’t verify';
    case 'pending':
      return 'Checking…';
  }
}

/** 説明の一文。`tag` ならタグの言い方（コミッターでなくタガー） */
export function signatureExplanation(status: SignatureStatus, tag = false): string {
  const what = tag ? 'tag' : 'commit';
  const who = tag ? 'tagger' : 'committer';
  switch (status.kind) {
    case 'verified':
      return (status.trust ?? 3) >= 3
        ? `This ${what} has a valid signature from a trusted key.`
        : `This ${what} has a valid signature. The key’s trust is marginal (partly trusted).`;
    case 'untrusted':
      return 'The signature is valid, but the key’s trust isn’t set. Check who owns the key and set its trust to make it Verified.';
    case 'signerMismatch':
      return `The signature is valid, but the ${who}’s email address isn’t listed on the signing key. Someone other than the ${who} may have signed it.`;
    case 'expired':
      return 'The signature or the key has expired. It may have been valid when it was signed.';
    case 'revoked':
      return 'The signing key has been revoked. This signature can’t be trusted.';
    case 'bad':
      return `The signature is invalid. The ${what} was changed after it was signed, or the signature is corrupt.`;
    case 'noKey':
      return 'There’s no public key to verify the signature (it isn’t in the GPG keyring or the SSH allowed signers).';
    case 'unsupported':
      return 'This git can’t verify SSH signatures. It needs git 2.34 or later.';
    case 'pending':
      return 'Verifying the signature.';
  }
}

/**
 * アイコン（codicon の名前）と色（テーマの色の ID）。アプリの SF Symbols と色に近いもの。信用度は形でも分かる
 * （塗りつぶしは強い信用、縁だけは弱い信用）
 */
export function signatureIcon(status: SignatureStatus): { icon: string; color?: string } {
  switch (status.kind) {
    case 'verified':
      return { icon: (status.trust ?? 3) >= 3 ? 'verified-filled' : 'verified', color: 'charts.green' };
    case 'untrusted':
      return { icon: 'unverified', color: 'charts.yellow' };
    case 'signerMismatch':
      return { icon: 'person', color: 'charts.yellow' };
    case 'expired':
      return { icon: 'history', color: 'charts.orange' };
    case 'revoked':
      return { icon: 'shield', color: 'charts.red' };
    case 'bad':
      return { icon: 'error', color: 'charts.red' };
    case 'noKey':
      return { icon: 'question' };
    case 'unsupported':
      return { icon: 'info' };
    case 'pending':
      return { icon: 'loading~spin' };
  }
}
