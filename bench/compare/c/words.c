// C has no standard hash map; this one is the shortest honest open-addressing table.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { char *word; long count; } Entry;

#define CAP (1 << 16)
static Entry table[CAP];

static unsigned long hash(const char *s) {
    unsigned long h = 14695981039346656037UL;
    while (*s) h = (h ^ (unsigned char)*s++) * 1099511628211UL;
    return h;
}

static int by_count(const void *x, const void *y) {
    const Entry *a = x, *b = y;
    if (a->count != b->count) return a->count > b->count ? -1 : 1;
    return strcmp(a->word, b->word);
}

int main(int argc, char **argv) {
    FILE *f = fopen(argv[1], "rb");
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    rewind(f);
    char *text = malloc(size + 1);
    fread(text, 1, size, f);
    text[size] = 0;
    fclose(f);

    long distinct = 0;
    for (char *w = strtok(text, " \n"); w; w = strtok(NULL, " \n")) {
        unsigned long i = hash(w) & (CAP - 1);
        while (table[i].word && strcmp(table[i].word, w) != 0) i = (i + 1) & (CAP - 1);
        if (!table[i].word) { table[i].word = w; distinct++; }
        table[i].count++;
    }

    Entry *all = malloc(distinct * sizeof(Entry));
    long n = 0;
    for (long i = 0; i < CAP; i++)
        if (table[i].word) all[n++] = table[i];
    qsort(all, n, sizeof(Entry), by_count);
    printf("%ld\n", distinct);
    for (int i = 0; i < 10; i++) printf("%s %ld\n", all[i].word, all[i].count);
    return 0;
}
