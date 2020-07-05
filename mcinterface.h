extern void print(int value);

enum Block {
    AIR,
    COBBLESTONE,
    GRANITE,
    ANDESITE,
    DIORITE,
    LAPIS_BLOCK,
    IRON_BLOCK,
    GOLD_BLOCK,
    DIAMOND_BLOCK,
    REDSTONE_BLOCK,
};

extern void turtle_x(int value);
extern void turtle_y(int value);
extern void turtle_z(int value);

// Sets the block at the turtle's position
extern void turtle_set(enum Block block);

// Returns 1 if the block at the turtle's position matches the argument
extern int turtle_check(enum Block block);

// Returns the block at the turtle's position
extern enum Block turtle_get(void);

extern int turtle_get_char(void);