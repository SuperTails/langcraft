#include "mcinterface.h"

int global_foo = 20;

void modify_foo(void);

int main() {
    modify_foo();
    print(global_foo);
}
