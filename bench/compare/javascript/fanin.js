// Concurrency: P producer async tasks (8 by default) each send 1,000,000 / P integers into one channel of
// capacity 1,024; one consumer sums them.
const { Chan } = require("./chan.js");
const producers = process.argv[2] ? parseInt(process.argv[2]) : 8;
const each = Math.floor(1000000 / producers);
const c = new Chan(1024);
async function produce(from) {
    for (let i = from; i < from + each; i++) await c.send(i);
}
async function main() {
    const ps = [];
    for (let p = 0; p < producers; p++) ps.push(produce(p * each));
    Promise.all(ps).then(() => c.close());
    let sum = 0;
    for (;;) {
        const v = await c.recv();
        if (v === undefined) break;
        sum += v;
    }
    console.log(String(sum));
}
main();
