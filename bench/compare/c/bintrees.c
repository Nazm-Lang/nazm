#include <stdio.h>
#include <stdlib.h>

typedef struct Node { struct Node *left, *right; } Node;

static Node *make(int depth) {
    Node *n = malloc(sizeof(Node));
    if (depth > 0) {
        n->left = make(depth - 1);
        n->right = make(depth - 1);
    } else {
        n->left = n->right = NULL;
    }
    return n;
}

static long check(const Node *n) { return n->left ? 1 + check(n->left) + check(n->right) : 1; }

static void release(Node *n) {
    if (n->left) {
        release(n->left);
        release(n->right);
    }
    free(n);
}

int main(int argc, char **argv) {
    int min_depth = 4;
    int max_depth = argc > 1 ? atoi(argv[1]) : 10;
    if (max_depth < min_depth + 2) max_depth = min_depth + 2;
    int stretch = max_depth + 1;
    Node *s = make(stretch);
    printf("stretch tree of depth %d\t check: %ld\n", stretch, check(s));
    release(s);
    Node *long_lived = make(max_depth);
    for (int d = min_depth; d <= max_depth; d += 2) {
        long iterations = 1L << (max_depth - d + min_depth);
        long total = 0;
        for (long i = 0; i < iterations; i++) {
            Node *t = make(d);
            total += check(t);
            release(t);
        }
        printf("%ld\t trees of depth %d\t check: %ld\n", iterations, d, total);
    }
    printf("long lived tree of depth %d\t check: %ld\n", max_depth, check(long_lived));
    release(long_lived);
    return 0;
}
