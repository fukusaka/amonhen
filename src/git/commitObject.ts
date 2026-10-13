/** `cat-file` で取ったコミットオブジェクトのパース */

export interface Signature {
  name: string;
  email: string;
  /** unix 秒 */
  date: number;
}

export interface CommitObject {
  tree: string;
  parents: string[];
  author: Signature;
  committer: Signature;
  /** subject と本文 */
  message: string;
}

const SIGNATURE = /^(.*) <(.*)> (\d+) [+-]\d{4}$/;

function parseSignature(value: string): Signature {
  const m = SIGNATURE.exec(value);
  return m ? { name: m[1]!, email: m[2]!, date: Number(m[3]) } : { name: value, email: '', date: 0 };
}

/**
 * ヘッダーは空行まで。gpgsig などの複数行のヘッダーは、続きの行が空白で始まる。
 */
export function parseCommitObject(text: string): CommitObject {
  const blank = text.indexOf('\n\n');
  const header = blank < 0 ? text : text.slice(0, blank);
  const message = blank < 0 ? '' : text.slice(blank + 2);
  const commit: CommitObject = {
    tree: '',
    parents: [],
    author: { name: '', email: '', date: 0 },
    committer: { name: '', email: '', date: 0 },
    message,
  };
  for (const line of header.split('\n')) {
    if (line.startsWith(' ')) {
      continue;
    }
    const sp = line.indexOf(' ');
    const key = line.slice(0, sp);
    const value = line.slice(sp + 1);
    switch (key) {
      case 'tree':
        commit.tree = value;
        break;
      case 'parent':
        commit.parents.push(value);
        break;
      case 'author':
        commit.author = parseSignature(value);
        break;
      case 'committer':
        commit.committer = parseSignature(value);
        break;
    }
  }
  return commit;
}
