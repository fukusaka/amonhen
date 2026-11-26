/**
 * 今からどれだけ前か（「3 hr. ago」）。アプリの RelativeDateTimeFormatter（unitsStyle .short）と同じく、いちばん大きな
 * 単位で切り捨てて数える。Reflog の行と Range Diff の前の版に使う。vscode と DOM に依存しない
 */
const format = new Intl.RelativeTimeFormat('en', { numeric: 'always', style: 'short' });

const DAY = 86_400;

/** `seconds`（unix 秒）が `now`（ミリ秒）からどれだけ前か。先の時刻なら「in …」 */
export function relativeTime(seconds: number, now = Date.now()): string {
  const delta = seconds - Math.floor(now / 1000);
  const ago = Math.abs(delta);
  const sign = delta < 0 ? -1 : 1;
  const [value, unit]: [number, Intl.RelativeTimeFormatUnit] =
    ago < 60
      ? [ago, 'second']
      : ago < 3600
        ? [ago / 60, 'minute']
        : ago < DAY
          ? [ago / 3600, 'hour']
          : ago < 7 * DAY
            ? [ago / DAY, 'day']
            : ago < 30.4375 * DAY
              ? [ago / (7 * DAY), 'week']
              : ago < 365.25 * DAY
                ? [ago / (30.4375 * DAY), 'month']
                : [ago / (365.25 * DAY), 'year'];
  // 0 は前として書く（+0 は「in 0 sec.」、-0 は「0 sec. ago」）
  const n = Math.floor(value);
  return format.format(n === 0 ? -0 : sign * n, unit);
}
