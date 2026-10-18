// Concurrency: memory per live task — N async tasks (100,000 by default) all awaiting one gate at once;
// main waits until every one has started, then opens the gate and they end.
const n = process.argv[2] ? parseInt(process.argv[2]) : 100000;
let started = 0;
let open;
const gate = new Promise((r) => (open = r));
async function task() {
    started += 1;
    await gate;
}
async function main() {
    const ps = [];
    for (let i = 0; i < n; i++) ps.push(task());
    while (started < n) await new Promise((r) => setTimeout(r, 1));
    open();
    await Promise.all(ps);
    console.log(started);
}
main();
