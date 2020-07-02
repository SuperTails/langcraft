extern void print(int value);

enum Block {
    AIR,
    COBBLESTONE,
    GRANITE,
    ANDESITE,
    DIORITE,
};

extern void turtle_x(int value);
extern void turtle_y(int value);
extern void turtle_z(int value);

extern void turtle_set(enum Block block);