#include "mcinterface.h"

void print_result(int func(int)) {
    print(func(41));
}

int add_1(int num) {
    return num + 1;
}

int main() {
    print_result(add_1);
}
