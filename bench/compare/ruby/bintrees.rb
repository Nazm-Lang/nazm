def make(depth)
  depth > 0 ? [make(depth - 1), make(depth - 1)] : nil
end

def check(tree)
  tree.nil? ? 1 : 1 + check(tree[0]) + check(tree[1])
end

min_depth = 4
max_depth = [(ARGV[0] || 10).to_i, min_depth + 2].max
stretch = max_depth + 1
puts "stretch tree of depth #{stretch}\t check: #{check(make(stretch))}"
long_lived = make(max_depth)
min_depth.step(max_depth, 2) do |d|
  iterations = 1 << (max_depth - d + min_depth)
  total = 0
  iterations.times { total += check(make(d)) }
  puts "#{iterations}\t trees of depth #{d}\t check: #{total}"
end
puts "long lived tree of depth #{max_depth}\t check: #{check(long_lived)}"
