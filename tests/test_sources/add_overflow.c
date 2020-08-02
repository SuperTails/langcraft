#include "mcinterface.h"

int main() {
    unsigned char c = 0xAA;
    c += 0x80;
    print((int)c);
}
