# Concurrency: spawn N asyncio tasks (1,000,000 by default) on the one event loop, each adding 1 to a
# shared counter; gather waits for all of them.
import asyncio
import sys

n = int(sys.argv[1]) if len(sys.argv) > 1 else 1000000
done = 0


async def task():
    global done
    done += 1


async def main():
    ts = [asyncio.ensure_future(task()) for _ in range(n)]
    await asyncio.gather(*ts)
    print(done)


asyncio.run(main())
