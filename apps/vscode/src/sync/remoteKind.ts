/**
 * リモートの URL の先が何か（Add Remote のシートの「Points to」）: GitHub・GitLab などのサービス、パス、SSH 先、それ
 * 以外はホスト名。アプリの `RemoteSheet.kind(of:)` と同じ規則と文。vscode に依存しない（Webview でも使う）
 */
export function remoteKind(url: string): string {
  const lower = url.toLowerCase();
  for (const [host, name] of [
    ['github.com', 'GitHub'],
    ['gitlab', 'GitLab'],
    ['bitbucket', 'Bitbucket'],
    ['codeberg', 'Codeberg'],
  ] as const) {
    if (lower.includes(host)) return name;
  }
  if (['/', './', '../', '~', 'file://'].some((p) => lower.startsWith(p))) return 'A path';
  // Windows のドライブ（C:/x・C:\x）は git でもパス
  if (/^[A-Za-z]:/.test(url)) return 'A path';
  if (lower.startsWith('ssh://') || lower.startsWith('git+ssh://')) {
    const host = hostOf(url);
    if (host) return `SSH host ${host}`;
  }
  // scp の形（user@host:path）
  const colon = url.indexOf(':');
  if (!lower.includes('://') && colon >= 0) {
    const left = url.slice(0, colon);
    return `SSH host ${left.split('@').pop() ?? left}`;
  }
  const host = hostOf(url);
  if (host) return host;
  return url === '' ? '' : 'Other';
}

/** URL のホスト名（読めなければ undefined） */
function hostOf(url: string): string | undefined {
  try {
    return new URL(url).hostname || undefined;
  } catch {
    return undefined;
  }
}
