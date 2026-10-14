#include <algorithm>
#include <fstream>
#include <iostream>
#include <string>
#include <unordered_map>
#include <vector>

int main(int argc, char **argv) {
    std::ifstream in(argv[1]);
    std::unordered_map<std::string, long> counts;
    std::string w;
    while (in >> w) counts[w]++;
    std::vector<std::pair<std::string, long>> all(counts.begin(), counts.end());
    std::sort(all.begin(), all.end(), [](auto &a, auto &b) {
        return a.second != b.second ? a.second > b.second : a.first < b.first;
    });
    std::cout << counts.size() << "\n";
    for (int i = 0; i < 10; i++) std::cout << all[i].first << " " << all[i].second << "\n";
}
