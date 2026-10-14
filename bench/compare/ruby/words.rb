counts = File.read(ARGV[0]).split.tally
puts counts.size
counts.sort_by { |w, n| [-n, w] }.first(10).each { |w, n| puts "#{w} #{n}" }
