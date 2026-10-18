// Concurrency: spawn N async tasks (1,000,000 by default) on the one event loop, each adding 1 to a
// shared counter; Promise.all waits for all of them.
const n = process.argv[2] ? parseInt(process.argv[2]) : 1000000;
let done = 0;
async function task() {
    done += 1;
}
async function main() {
    const ps = new Array(n);
    for (let i = 0; i < n; i++) ps[i] = task();
    await Promise.all(ps);
    console.log(done);
}
main();
