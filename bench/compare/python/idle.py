# Concurrency: memory per live task — N asyncio tasks (100,000 by default) all awaiting one event at
# once; main waits until every one has started, then sets the event and they end.
import asyncio
import sys

n = int(sys.argv[1]) if len(sys.argv) > 1 else 100000
started = 0


async def task(gate):
    global started
    started += 1
    await gate.wait()


async def main():
    gate = asyncio.Event()
    ts = [asyncio.ensure_future(task(gate)) for _ in range(n)]
    while started < n:
        await asyncio.sleep(0.001)
    gate.set()
    await asyncio.gather(*ts)
    print(started)


asyncio.run(main())
