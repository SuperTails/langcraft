#include "mcinterface.h"

extern int global_foo;

void modify_foo(void) {
    global_foo += 22;
}