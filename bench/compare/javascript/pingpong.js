// Concurrency: N round trips (100,000 by default) between two async tasks over two channels of capacity 1.
const { Chan } = require("./chan.js");
const n = process.argv[2] ? parseInt(process.argv[2]) : 100000;
const ping = new Chan(1);
const pong = new Chan(1);
async function echo() {
    for (;;) {
        const v = await ping.recv();
        if (v === undefined) break;
        await pong.send(v + 1);
    }
}
async function driver() {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        await ping.send(i);
        sum += await pong.recv();
    }
    ping.close();
    return sum;
}
Promise.all([echo(), driver()]).then((r) => console.log(String(r[1])));
