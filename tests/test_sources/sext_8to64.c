#include <mcinterface.h>

char a = 127;
char b = -128;

int main() {
    print((long int) a);
    print((unsigned long int) ((long int) a) >> 32);
    print((long int) b);
    print((unsigned long int) ((long int) b) >> 32);
}
