// Concurrency: P producer tasks (8 by default) each send 1,000,000 / P integers into one bounded channel
// of capacity 1,024; main sums them.
using System.Threading.Channels;
using System.Threading.Tasks;

int producers = args.Length > 0 ? int.Parse(args[0]) : 8;
int each = 1000000 / producers;
var c = Channel.CreateBounded<long>(1024);
var ps = new Task[producers];
for (int p = 0; p < producers; p++)
{
    long from = (long)p * each;
    ps[p] = Task.Run(async () =>
    {
        for (long i = from; i < from + each; i++) await c.Writer.WriteAsync(i);
    });
}
_ = Task.WhenAll(ps).ContinueWith(_ => c.Writer.Complete());
long sum = 0;
await foreach (var v in c.Reader.ReadAllAsync()) sum += v;
Console.WriteLine(sum);
