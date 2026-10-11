# The same hand-off as `bench/programs/channels.nz`, statement for statement (Gate 4 light).
#
# One producer, one consumer, a queue of capacity one, so every message is a synchronisation.
# CPython's tasks are OS threads under one interpreter lock; a closed channel is a sentinel.
import queue
import threading

CLOSED = None


def produce(jobs):
    i = 1
    while i <= 50000:
        jobs.put(i)
        i = i + 1


def consume(jobs, out):
    total = 0
    while True:
        v = jobs.get()
        if v is CLOSED:
            break
        total = total + v
    out.put(total)


def main():
    jobs = queue.Queue(maxsize=1)
    out = queue.Queue(maxsize=1)
    consumer = threading.Thread(target=consume, args=(jobs, out))
    consumer.start()
    producer = threading.Thread(target=produce, args=(jobs,))
    producer.start()
    producer.join()
    jobs.put(CLOSED)
    consumer.join()
    print(out.get())


main()
