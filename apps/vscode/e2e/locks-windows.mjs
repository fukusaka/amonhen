/**
 * Windows の e2e で、git の鍵のファイル（`index.lock`・`config.lock`）を誰が作り・開き・消したかを OS で記録する。
 * CI でまれに鍵とぶつかって落ちる件で、鍵を持っていたのが誰か（git か、git 以外のプロセスか）を言うため。
 * Windows では git の trace2 を取ると e2e が遅くなりすぎる（env.mjs）うえ、trace2 は git しか記録しないので、
 * カーネルのファイルの出来事（ETW の Microsoft-Windows-Kernel-File）とプロセスの起動（Kernel-Process）を取る。
 * 記録は e2e の全体で、鍵とぶつかった試験は時刻を `AMONHEN_E2E_LOCK_LOG` に書く。終わってから、その前後の出来事を出す。
 * ETW の記録には管理者の権限が要る（CI の runner にはある）。取れなければそう言って、記録なしで進む。
 * ETW のプロセスの起動にはコマンドラインがないので、Windows の監査（Security のログの 4688、コマンドラインつき）も
 * 有効にし、鍵とぶつかったら、その前後の出来事に出てくるプロセスの命令を添える（どの git の命令が鍵を持っていたか）
 */
import { spawnSync } from 'node:child_process';
import { createReadStream, existsSync, readFileSync, statSync } from 'node:fs';
import { basename, join } from 'node:path';

const session = 'amonhen-e2e-locks';
// Kernel-File のキーワード: CREATE（開く）0x80・DELETE_PATH 0x400・RENAME_SETLINK_PATH 0x800・CREATE_NEW_FILE 0x1000
const fileKeywords = '0x1c80';
// Kernel-Process のキーワード: PROCESS（起動・終了）0x10
const processKeywords = '0x10';

const logman = (...args) => spawnSync('logman', args, { encoding: 'utf8' });

/** 記録を始める。始められたら、止めて結果を出す関数を返す */
export function recordLocks(dir) {
  const etl = join(dir, 'locks.etl');
  // 前に途中で終わった記録が残っていれば止める
  logman('stop', session, '-ets');
  const started = logman(
    'start', session, '-p', 'Microsoft-Windows-Kernel-File', fileKeywords, '0x5',
    '-o', etl, '-bs', '1024', '-nb', '64', '512', '-ets',
  );
  if (started.status !== 0) {
    console.log(`鍵のファイルの記録（ETW）を始められません: ${(started.stdout + started.stderr).trim()}`);
    return undefined;
  }
  const added = logman('update', 'trace', session, '-p', 'Microsoft-Windows-Kernel-Process', processKeywords, '0x5', '-ets');
  if (added.status !== 0) {
    console.log(`プロセスの起動の記録を足せません: ${(added.stdout + added.stderr).trim()}`);
  }
  auditProcesses();
  const began = Date.now();
  // 記録より前から動いているプロセスの名前
  const before = spawnSync('tasklist', ['/fo', 'csv', '/nh'], { encoding: 'utf8' }).stdout ?? '';
  return () => report(dir, etl, before, began);
}

/**
 * プロセスの作成の監査（4688）を、コマンドラインつきで有効にする。e2e は数千の git を起こすので、Security のログが
 * 古いものから消えないよう大きくする。使い捨ての CI の runner でだけ呼ぶ（run.mjs）
 */
function auditProcesses() {
  const steps = [
    ['reg', ['add', 'HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\System\\Audit',
      '/v', 'ProcessCreationIncludeCmdLine_Enabled', '/t', 'REG_DWORD', '/d', '1', '/f']],
    // Process Creation の副カテゴリ（名前は言語で変わるので GUID で）
    ['auditpol', ['/set', '/subcategory:{0CCE922B-69AE-11D9-BED3-505054503030}', '/success:enable']],
    ['wevtutil', ['sl', 'Security', '/ms:1073741824']],
  ];
  for (const [command, args] of steps) {
    const done = spawnSync(command, args, { encoding: 'utf8' });
    if (done.status !== 0) {
      console.log(`プロセスの命令の記録（監査）を始められません: ${command} ${(done.stdout + done.stderr).trim()}`);
      return;
    }
  }
}

/**
 * `from`〜`to`（ミリ秒）に作られたプロセスの、作られた時刻・番号・命令（監査の 4688）。読めなければ空
 */
function processCommands(from, to) {
  const iso = (ms) => new Date(ms).toISOString();
  const script =
    `Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4688; ` +
    `StartTime = [datetime]::Parse('${iso(from)}').ToLocalTime(); EndTime = [datetime]::Parse('${iso(to)}').ToLocalTime() } ` +
    `-ErrorAction SilentlyContinue | ForEach-Object { $x = [xml]$_.ToXml(); $d = @{}; ` +
    `foreach ($e in $x.Event.EventData.Data) { $d[$e.Name] = $e.'#text' }; ` +
    `'{0}\t{1}\t{2}' -f ([DateTimeOffset]$_.TimeCreated).ToUnixTimeMilliseconds(), [Convert]::ToInt64($d.NewProcessId, 16), ` +
    `($d.CommandLine -replace '\\s+', ' ') }`;
  const out = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    maxBuffer: 64 * 2 ** 20,
  });
  return (out.stdout ?? '')
    .split('\n')
    .map((line) => line.trimEnd().split('\t'))
    .filter((f) => f.length === 3)
    .map(([at, pid, command]) => ({ at: Number(at), pid, command }));
}

async function report(dir, etl, before, began) {
  const query = logman('query', session, '-ets').stdout ?? '';
  const lost = /Buffers Lost:\s*(\d+)/i.exec(query)?.[1];
  logman('stop', session, '-ets');
  // 記録の間ずっと動いていたプロセスの名前
  const after = spawnSync('tasklist', ['/fo', 'csv', '/nh'], { encoding: 'utf8' }).stdout ?? '';
  const failures = existsSync(join(dir, 'lock-failures.txt'))
    ? readFileSync(join(dir, 'lock-failures.txt'), 'utf8').split('\n').filter(Boolean)
    : [];
  const size = `${(statSync(etl).size / 2 ** 20).toFixed(0)} MB、取りこぼした buffer ${lost ?? '?'}`;
  if (failures.length === 0) {
    // 読むのに数十秒かかるので、鍵とぶつからなかったら読まない
    console.log(`鍵のファイルの記録（ETW）: ${size}。鍵とぶつかった試験はありません`);
    // 命令の記録が効いているかを、ぶつからなかったときも確かめる（終わりの 1 分の git の数と 1 つの例。全体を読むと遅い）
    const gits = processCommands(Math.max(began, Date.now() - 60_000), Date.now()).filter((c) => /\bgit(\.exe)?"?\s/i.test(c.command));
    console.log(`プロセスの命令の記録（監査の 4688）: git ${gits.length} 件${gits[0] ? `、例: ${gits[0].command.slice(0, 200)}` : ''}`);
    return;
  }
  const start = performance.now();
  const xml = join(dir, 'locks.xml');
  const converted = spawnSync('tracerpt', [etl, '-o', xml, '-of', 'XML', '-y'], { encoding: 'utf8' });
  if (converted.status !== 0) {
    console.log(`鍵のファイルの記録を読めません: ${(converted.stdout + converted.stderr).trim()}`);
    return;
  }
  const names = new Map();
  for (const line of `${after}\n${before}`.split('\n')) {
    const [image, pid] = line.split('","');
    if (pid) names.set(pid, image.replace(/^"/, ''));
  }
  const parents = new Map();
  const events = [];
  // 一時ディレクトリの中の鍵だけ（ほかの git の鍵は見ない）
  const mark = basename(dir).toLowerCase();
  const chainOf = (pid) => {
    const chain = [];
    for (let p = pid, i = 0; p && i < 3; p = parents.get(p), i++) chain.push(p);
    return chain;
  };
  const label = (pid) => chainOf(pid).map((p) => `${names.get(p) ?? '?'}(${p})`).join(' ← ');
  for await (const event of eachEvent(xml)) {
    const id = tag(event, 'EventID');
    const pid = /ProcessID="(\d+)"/.exec(event)?.[1];
    if (event.includes('Microsoft-Windows-Kernel-Process')) {
      const child = data(event, 'ProcessID');
      const image = data(event, 'ImageName');
      if (id === '1' && child && image) {
        names.set(child, basename(image.replaceAll('\\', '/')));
        parents.set(child, data(event, 'ParentProcessID'));
      }
      continue;
    }
    const path = data(event, 'FileName') ?? data(event, 'FilePath');
    if (!path || !/(index|config)\.lock$/i.test(path) || !path.toLowerCase().includes(mark)) continue;
    const time = /SystemTime="([^"]+)"/.exec(event)?.[1];
    // 開いたものは共有のしかた（FILE_SHARE_DELETE の 4 がないと、閉じるまで git が鍵を消せない）
    const share = id === '12' ? data(event, 'ShareAccess') : undefined;
    // pid は使い回されるので、名前はその時点のものを引く
    const at = time ? Date.parse(time.replace(/(\.\d{3})\d*/, '$1')) : NaN;
    events.push({ at, id, path, share, who: label(pid), pids: chainOf(pid) });
  }
  const kinds = { 12: '開く', 26: '消す', 27: '名前を変える', 30: '作る' };
  console.log(
    `鍵のファイルの記録（ETW）: ${size}、鍵の出来事 ${events.length}、` +
      `読むのに ${((performance.now() - start) / 1000).toFixed(1)}s`,
  );
  for (const failure of failures) {
    const at = Number(failure.split(' ', 1)[0]);
    console.log(`鍵とぶつかった試験: ${failure.slice(failure.indexOf(' ') + 1)}`);
    const near = events.filter((e) => e.at >= at - 5000 && e.at <= at + 500);
    // 同じ時刻・同じプロセス・同じ出来事は 1 行にまとめる
    const lines = [];
    for (const e of near) {
      const where = e.path.slice(e.path.toLowerCase().indexOf(mark) + mark.length + 1);
      const share = e.share ? ` 共有 ${e.share}` : '';
      const line = `${e.at - at} ms ${kinds[e.id] ?? `出来事 ${e.id}`} ${where}${share} ${e.who}`;
      const last = lines.at(-1);
      if (last?.line === line) last.count++;
      else lines.push({ line, count: 1 });
    }
    for (const { line, count } of lines) console.log(`  ${line}${count > 1 ? ` ×${count}` : ''}`);
    if (near.length === 0) console.log('  前後 5 秒に鍵の出来事はありません');
    // 出来事に出てきたプロセス（と親）の命令。番号は使い回されるので、出来事より前に作られた最後のものを引く
    const created = processCommands(at - 60_000, at + 1000);
    const shown = new Map();
    for (const e of near) {
      for (const pid of e.pids) {
        if (shown.has(pid)) continue;
        const made = created.filter((c) => c.pid === pid && c.at <= e.at + 50).at(-1);
        shown.set(pid, made?.command);
      }
    }
    const commands = [...shown].filter(([, command]) => command);
    if (commands.length > 0) {
      console.log('  プロセスの命令（監査の 4688）:');
      for (const [pid, command] of commands) console.log(`    ${pid}: ${command.slice(0, 400)}`);
    } else {
      console.log(`  プロセスの命令は監査の記録にありません（作られた記録 ${created.length}）`);
    }
  }
}

/** tracerpt の XML を `<Event ...>…</Event>` ごとに読む（全体は大きいので流しながら） */
async function* eachEvent(file) {
  let rest = '';
  for await (const chunk of createReadStream(file, { encoding: 'utf8' })) {
    rest += chunk;
    let end;
    while ((end = rest.indexOf('</Event>')) >= 0) {
      const begin = rest.lastIndexOf('<Event ', end);
      if (begin >= 0) yield rest.slice(begin, end);
      rest = rest.slice(end + 8);
    }
  }
}

const tag = (event, name) => new RegExp(`<${name}[^>]*>([^<]*)</${name}>`).exec(event)?.[1];
// 数は桁をそろえるために前に空白が入る
const data = (event, name) => new RegExp(`<Data Name="${name}">([^<]*)</Data>`).exec(event)?.[1].trim();
