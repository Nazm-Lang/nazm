import { readFileSync } from "fs";

const counts = new Map<string, number>();
for (const w of readFileSync(process.argv[2], "utf8").split(/[ \n]+/)) {
  if (w) counts.set(w, (counts.get(w) ?? 0) + 1);
}
const all = [...counts].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1));
console.log(counts.size);
for (const [w, n] of all.slice(0, 10)) console.log(`${w} ${n}`);
