#include "mcinterface.h"

#define uint32_t unsigned int

/*void llvm_fshr_i32(uint32_t hi, uint32_t lo, uint32_t raw_amount) {
    uint32_t amount = raw_amount % 32;

    uint32_t result;
    if (amount == 0) {
        result = lo;
    } else {
        uint32_t mask = (1 << amount) - 1;
        uint32_t temp = (hi & mask) << (32 - amount);
        result = (lo >> amount) + temp;
    }

    print(result);
}*/

/*void mul_32_to_64(uint32_t op1, uint32_t op2) {
    uint32_t u1 = (op1 & 0xffff);
    uint32_t v1 = (op2 & 0xffff);
    uint32_t t = (u1 * v1);
    uint32_t w3 = (t & 0xffff);
    uint32_t k = (t >> 16);

    op1 >>= 16;
    t = (op1 * v1) + k;
    k = (t & 0xffff);
    uint32_t w1 = (t >> 16);

    op2 >>= 16;
    t = (u1 * op2) + k;
    k = (t >> 16);

    uint32_t hi = (op1 * op2) + w1 + k;
    uint32_t lo = (t << 16) + w3;
    print(hi);
    print(lo);
}*/

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
