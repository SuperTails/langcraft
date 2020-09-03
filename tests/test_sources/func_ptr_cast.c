#include "mcinterface.h"

struct Foo {
    int x;
};

struct Bar {
    int x;
};

void print_foo(struct Foo *foo) {
    print(foo->x);
}

void (*print_bar)(struct Bar *) = (void(*)(struct Bar *))print_foo;

int main() {
    struct Foo foo;
    foo.x = 42;
    print_foo(&foo);
    print_bar(((struct Bar*)(&foo)));
}
