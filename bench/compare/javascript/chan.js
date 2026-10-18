// A bounded channel for async functions: send waits while it is full, recv waits while it is empty;
// close() ends the receivers once the buffer drains.
class Chan {
    constructor(cap) {
        this.cap = cap;
        this.q = [];
        this.closed = false;
        this.recvWaiters = [];
        this.sendWaiters = [];
    }
    async send(v) {
        while (this.q.length >= this.cap) await new Promise((r) => this.sendWaiters.push(r));
        this.q.push(v);
        const w = this.recvWaiters.shift();
        if (w) w();
    }
    async recv() {
        while (this.q.length === 0) {
            if (this.closed) return undefined;
            await new Promise((r) => this.recvWaiters.push(r));
        }
        const v = this.q.shift();
        const w = this.sendWaiters.shift();
        if (w) w();
        return v;
    }
    close() {
        this.closed = true;
        for (const w of this.recvWaiters.splice(0)) w();
    }
}
module.exports = { Chan };
