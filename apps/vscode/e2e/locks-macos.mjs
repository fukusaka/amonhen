/**
 * macOS の e2e で、git の鍵のファイル（`index.lock`・`config.lock`）を誰が作り・名前を変え・消したかを OS で記録する。
 * CI でまれに鍵とぶつかって落ちる件で、trace2 には鍵を取りうる git が出ない（git 以外か、記録に出ない git）ので、
 * Endpoint Security の出来事を `eslogger` で取る。`fs_usage` は出来事が多いと何分も抜け落ちた（CI で測った）ので使わない。
 * 鍵とぶつかった試験は時刻を `AMONHEN_E2E_LOCK_LOG` に書き、終わってから、その前後の出来事を出す（locks-windows.mjs と同じ）。
 * `eslogger` には root が要る（CI の runner は sudo にパスワードが要らない）。使えなければそう言って、記録なしで進む
 */
import { spawn, spawnSync } from 'node:child_process';
import { createReadStream, existsSync, readFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { createInterface } from 'node:readline';

const lockFile = /(index|config)\.lock/;
// 開く・閉じるまで取ると出来事が多すぎる。macOS では開いているファイルも消せる・名前を変えられるので、鍵の持ち主は
// 作ったプロセスで足りる
const kinds = { create: '作る', rename: '名前を変える', unlink: '消す' };

/** 記録を始める。始められたら、止めて結果を出す関数を返す */
export function recordLocks(dir) {
  if (spawnSync('sudo', ['-n', 'true']).status !== 0) {
    console.log('鍵のファイルの記録（eslogger）を始められません: sudo にパスワードが要る');
    return undefined;
  }
  // 受け手を挟まずにファイルへ書かせ、終わってから読む
  const raw = join(dir, 'eslogger.jsonl');
  // eslogger は自分のプロセスグループの出来事を捨てる（堂々巡りを避けるため。man eslogger）。run.mjs のグループには
  // VSCode・拡張ホスト・試験の git まで入るので、detached で別のグループにする
  const child = spawn('sudo', ['-n', 'sh', '-c', 'exec eslogger "$@" > "$0"', raw, ...Object.keys(kinds)], {
    stdio: ['ignore', 'ignore', 'pipe'],
    detached: true,
  });
  let errors = '';
  child.stderr.on('data', (d) => (errors += d));
  return async () => {
    // sudo は受けた signal を eslogger に渡す
    const exited = new Promise((resolve) => child.once('exit', resolve));
    child.kill('SIGINT');
    await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 5000))]);
    await report(dir, raw, errors);
  };
}

async function report(dir, raw, errors) {
  // 一時ディレクトリの中の鍵だけ（JSON では `/` が `\/` になるので、ディレクトリの名前で見る）
  const mark = basename(dir);
  const events = [];
  // プロセスの名前と親（鍵に触らない親の名前も引けるように、すべての出来事から集める）
  const names = new Map();
  const parents = new Map();
  // 出来事の種類ごとの通し番号（抜けた数が取りこぼした数）
  const seqs = new Map();
  let lines = 0;
  if (existsSync(raw)) {
    for await (const line of createInterface({ input: createReadStream(raw) })) {
      let e;
      try {
        e = JSON.parse(line);
      } catch {
        continue;
      }
      lines++;
      const pid = e.process?.audit_token?.pid;
      names.set(pid, basename(e.process?.executable?.path ?? '?'));
      parents.set(pid, e.process?.ppid);
      const seq = seqs.get(e.event_type) ?? { min: e.seq_num, max: e.seq_num, count: 0 };
      seq.min = Math.min(seq.min, e.seq_num);
      seq.max = Math.max(seq.max, e.seq_num);
      seq.count++;
      seqs.set(e.event_type, seq);
      if (!line.includes(mark) || !lockFile.test(line)) continue;
      const kind = Object.keys(e.event ?? {})[0];
      const body = e.event?.[kind] ?? {};
      const path =
        body.target?.path ??
        body.source?.path ??
        body.destination?.existing_file?.path ??
        (body.destination?.new_path ? `${body.destination.new_path.dir?.path}/${body.destination.new_path.filename}` : undefined);
      if (!path || !lockFile.test(path)) continue;
      events.push({ at: Date.parse(e.time), kind, path, pid, ppid: e.process?.ppid });
    }
  }
  const lost = [...seqs.values()].reduce((n, s) => n + (s.max - s.min + 1 - s.count), 0);
  const failures = existsSync(join(dir, 'lock-failures.txt'))
    ? readFileSync(join(dir, 'lock-failures.txt'), 'utf8').split('\n').filter(Boolean)
    : [];
  console.log(
    `鍵のファイルの記録（eslogger）: ${lines} 行、取りこぼし ${lost}、鍵の出来事 ${events.length}` +
      (errors.trim() ? `、エラー: ${errors.trim().split('\n').slice(0, 3).join(' / ')}` : '') +
      (failures.length === 0 ? '。鍵とぶつかった試験はありません' : ''),
  );
  const label = (pid, ppid) => {
    const chain = [`${names.get(pid) ?? '?'}(${pid})`];
    for (let p = ppid, i = 0; p && i < 2; p = parents.get(p), i++) chain.push(`${names.get(p) ?? '?'}(${p})`);
    return chain.join(' ← ');
  };
  for (const failure of failures) {
    const at = Number(failure.split(' ', 1)[0]);
    console.log(`鍵とぶつかった試験: ${failure.slice(failure.indexOf(' ') + 1)}`);
    // 同じ時刻・同じプロセス・同じ出来事は 1 行にまとめる
    const rows = [];
    for (const e of events.filter((e) => e.at >= at - 5000 && e.at <= at + 500)) {
      const where = e.path.slice(e.path.indexOf(mark) + mark.length + 1);
      const row = `${e.at - at} ms ${kinds[e.kind] ?? e.kind} ${where} ${label(e.pid, e.ppid)}`;
      const last = rows.at(-1);
      if (last?.row === row) last.count++;
      else rows.push({ row, count: 1 });
    }
    for (const { row, count } of rows) console.log(`  ${row}${count > 1 ? ` ×${count}` : ''}`);
    if (rows.length === 0) console.log('  前後 5 秒に鍵の出来事はありません');
  }
}
