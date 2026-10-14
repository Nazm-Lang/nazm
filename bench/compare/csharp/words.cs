using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;

var counts = new Dictionary<string, int>();
foreach (var w in File.ReadAllText(args[0]).Split(new[] { ' ', '\n' }, StringSplitOptions.RemoveEmptyEntries))
    counts[w] = counts.GetValueOrDefault(w) + 1;
Console.WriteLine(counts.Count);
foreach (var e in counts.OrderByDescending(e => e.Value).ThenBy(e => e.Key, StringComparer.Ordinal).Take(10))
    Console.WriteLine($"{e.Key} {e.Value}");
