// Concurrency: memory per live task — N tasks (100,000 by default) all awaiting one gate at once; main
// waits until every one has started, then opens the gate and they end. (Async tasks are stackless:
// a waiting one holds its state machine, not a thread.)
using System.Threading;
using System.Threading.Tasks;

int n = args.Length > 0 ? int.Parse(args[0]) : 100000;
var gate = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
long started = 0;
var tasks = new Task[n];
for (int i = 0; i < n; i++)
    tasks[i] = Task.Run(async () =>
    {
        Interlocked.Increment(ref started);
        await gate.Task;
    });
while (Interlocked.Read(ref started) < n) await Task.Delay(1);
gate.SetResult();
await Task.WhenAll(tasks);
Console.WriteLine(Interlocked.Read(ref started));
