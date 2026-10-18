// Concurrency: spawn N tasks (1,000,000 by default) on the thread pool, each adding 1 to a shared
// counter; Task.WhenAll waits for all of them.
using System.Threading;
using System.Threading.Tasks;

int n = args.Length > 0 ? int.Parse(args[0]) : 1000000;
long done = 0;
var tasks = new Task[n];
for (int i = 0; i < n; i++) tasks[i] = Task.Run(() => { Interlocked.Increment(ref done); });
await Task.WhenAll(tasks);
Console.WriteLine(Interlocked.Read(ref done));
