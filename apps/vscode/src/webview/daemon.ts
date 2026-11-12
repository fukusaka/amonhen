/**
 * メインビューの Daemon 画面（アプリの Activity の Daemon のタブと同じ）。手元の共有の daemon の様子と Start・Stop、
 * daemon が開いているリポジトリ、この窓の daemon とのつながり
 */
import { clientsCell, duration, indexCell, sshCell, statusLine, type DaemonHostMessage } from '../daemonStatus/protocol.ts';
import type { HostMessage } from '../view/protocol.ts';
import { el, post, type View } from './api.ts';

export function createDaemonView(): View {
  const root = el('div', 'view daemon');
  const content = el('div', 'daemon-content');
  root.append(content);
  let last: DaemonHostMessage | undefined;

  function table(heads: string[], rows: HTMLTableRowElement[], empty: string): HTMLTableElement {
    const t = el('table', 'settings-table');
    const head = el('tr');
    for (const h of heads) head.append(el('th', '', h));
    t.append(head);
    if (rows.length === 0) {
      const tr = el('tr');
      const td = el('td', 'muted', empty);
      td.colSpan = heads.length;
      tr.append(td);
      rows = [tr];
    }
    t.append(...rows);
    return t;
  }

  function render(m: DaemonHostMessage): void {
    const s = m.status;
    const header = el('div', 'daemon-header');
    const text = el('div', 'daemon-state');
    const headline = el('div', s ? 'daemon-headline running' : 'daemon-headline');
    headline.textContent = s
      ? '● The shared daemon is running'
      : `○ ${!m.checked ? 'Checking…' : m.enabled ? 'The shared daemon isn’t running' : 'The Amon Hen engine isn’t in use'}`;
    text.append(headline);
    if (s) {
      text.append(el('div', 'daemon-detail', statusLine(s)), el('div', 'daemon-detail muted', s.socket));
    } else if (m.checked) {
      text.append(
        el(
          'div',
          'daemon-detail muted',
          m.enabled
            ? 'It starts automatically when this window opens a repository.'
            : 'amonhen.daemon.enabled is off, or there is no engine for this machine. History and the rest run on git directly.',
        ),
      );
    }
    header.append(text);
    if (m.enabled) {
      const button = el('button', 'secondary', s ? 'Stop' : 'Start');
      if (s) button.title = 'Repositories in this window get disconnected (Start connects them again).';
      button.addEventListener('click', () => {
        button.disabled = true;
        post({ type: s ? 'daemonStop' : 'daemonStart' });
      });
      header.append(button);
    }

    const repos = (s?.repos ?? []).map((r) => {
      const tr = el('tr');
      const path = el('td', 'mono', r.path);
      path.title = r.path;
      const ssh = el('td', r.ssh && r.closed ? 'error' : r.ssh ? '' : 'muted', sshCell(r));
      if (r.ssh) ssh.title = r.closed ?? `Sent to and received from the SSH host${r.engine ? `\nEngine on the SSH host: ${r.engine}` : ''}`;
      tr.append(
        path,
        ssh,
        el('td', '', clientsCell(r)),
        el('td', r.history ? '' : 'muted', r.history ? 'Yes' : '—'),
        el('td', r.watching ? '' : 'muted', r.watching ? 'Yes' : '—'),
        el('td', r.indexRows == null ? 'muted' : '', indexCell(r)),
      );
      return tr;
    });
    const connections = m.connections.map((c) => {
      const tr = el('tr');
      const status = el('td', c.closed ? 'error' : 'ok', c.closed ? 'Disconnected' : 'Connected');
      if (c.closed) status.title = c.closed;
      tr.append(el('td', 'mono', c.path), el('td', 'muted', 'Local shared daemon'), status);
      return tr;
    });
    content.replaceChildren(
      header,
      el('h3', '', 'Open Repositories'),
      table(['Repository', 'SSH', 'Connections', 'History', 'Watching', 'Search Index'], repos, 'None'),
      el('h3', '', 'This Window’s Connections to the Daemon'),
      table(['Repository', 'Location', 'Status'], connections, 'None'),
      el(
        'div',
        'daemon-detail muted',
        `The daemon exits after ${duration(s?.idleExitMs ?? 600_000)} with no connections. From a terminal, run amonhen daemon status to see it.`,
      ),
    );
  }

  return {
    root,
    onMessage(message: HostMessage) {
      if (message.type !== 'daemonStatus') return;
      last = message;
      render(message);
    },
    onShow() {
      if (last) render(last);
    },
  };
}

