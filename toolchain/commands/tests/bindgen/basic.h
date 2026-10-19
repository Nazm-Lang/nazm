/* basic */
#ifndef BASIC_H
#define BASIC_H
#include <stdint.h>
#pragma once

int64_t add(int64_t a, int64_t b);
bool ok(bool x);
_Bool flag(_Bool);
const char *name(void);
char const *other(int64_t n);
int64_t greet(const char *who);
int64_t noparams(void);
int64_t emptyparams();
int64_t _hidden(int64_t _x, int64_t y);
#endif
