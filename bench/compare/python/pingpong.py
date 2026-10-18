# Concurrency: N round trips (100,000 by default) between two asyncio tasks over two queues of capacity 1.
import asyncio
import sys

n = int(sys.argv[1]) if len(sys.argv) > 1 else 100000


async def echo(ping, pong):
    while True:
        v = await ping.get()
        if v < 0:
            break
        await pong.put(v + 1)


async def driver(ping, pong):
    total = 0
    for i in range(n):
        await ping.put(i)
        total += await pong.get()
    await ping.put(-1)
    return total


async def main():
    ping, pong = asyncio.Queue(1), asyncio.Queue(1)
    _, total = await asyncio.gather(echo(ping, pong), driver(ping, pong))
    print(total)


asyncio.run(main())
