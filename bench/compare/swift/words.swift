import Foundation

let text = try! String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
var counts: [Substring: Int] = [:]
for w in text.split(whereSeparator: { $0 == " " || $0 == "\n" }) {
    counts[w, default: 0] += 1
}
let all = counts.sorted { $0.value != $1.value ? $0.value > $1.value : $0.key < $1.key }
print(counts.count)
for (w, n) in all.prefix(10) {
    print("\(w) \(n)")
}
