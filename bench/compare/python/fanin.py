# Concurrency: P producer asyncio tasks (8 by default) each send 1,000,000 / P integers into one queue of
# capacity 1,024; one consumer sums them (each producer ends with a -1).
import asyncio
import sys

producers = int(sys.argv[1]) if len(sys.argv) > 1 else 8
each = 1000000 // producers


async def produce(q, start):
    for i in range(start, start + each):
        await q.put(i)
    await q.put(-1)


async def main():
    q = asyncio.Queue(1024)
    ts = [asyncio.ensure_future(produce(q, p * each)) for p in range(producers)]
    total = 0
    ended = 0
    while ended < producers:
        v = await q.get()
        if v < 0:
            ended += 1
        else:
            total += v
    await asyncio.gather(*ts)
    print(total)


asyncio.run(main())
