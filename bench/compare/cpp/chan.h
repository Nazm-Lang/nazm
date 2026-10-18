// A bounded channel (capacity cap) over a mutex and two condition variables; close() ends the receivers.
#pragma once
#include <condition_variable>
#include <deque>
#include <mutex>
template <class T>
class Chan {
    std::mutex m;
    std::condition_variable not_full, not_empty;
    std::deque<T> q;
    size_t cap;
    bool closed = false;
public:
    explicit Chan(size_t c) : cap(c) {}
    void send(T v) {
        std::unique_lock<std::mutex> l(m);
        not_full.wait(l, [&] { return q.size() < cap; });
        q.push_back(v);
        not_empty.notify_one();
    }
    bool recv(T &out) {
        std::unique_lock<std::mutex> l(m);
        not_empty.wait(l, [&] { return !q.empty() || closed; });
        if (q.empty()) return false;
        out = q.front();
        q.pop_front();
        not_full.notify_one();
        return true;
    }
    void close() {
        std::lock_guard<std::mutex> l(m);
        closed = true;
        not_empty.notify_all();
    }
};
