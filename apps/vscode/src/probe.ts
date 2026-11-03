/** CI で時間を測るための一時的なログ（マージしない） */
export function probe(label: string): void {
  console.log(`[probe] ${new Date().toISOString().slice(11, 23)} ${label}`);
}
