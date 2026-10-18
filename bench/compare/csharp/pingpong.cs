// Concurrency: N round trips (100,000 by default) between two tasks over two bounded channels of
// capacity 1 (System.Threading.Channels); prints the sum of the values that came back.
using System.Threading.Channels;
using System.Threading.Tasks;

int n = args.Length > 0 ? int.Parse(args[0]) : 100000;
var ping = Channel.CreateBounded<long>(1);
var pong = Channel.CreateBounded<long>(1);
var echo = Task.Run(async () =>
{
    await foreach (var v in ping.Reader.ReadAllAsync()) await pong.Writer.WriteAsync(v + 1);
});
var driver = Task.Run(async () =>
{
    long sum = 0;
    for (long i = 0; i < n; i++)
    {
        await ping.Writer.WriteAsync(i);
        sum += await pong.Reader.ReadAsync();
    }
    ping.Writer.Complete();
    return sum;
});
Console.WriteLine(await driver);
await echo;
