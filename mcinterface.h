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

enum Block turtle_get() {
    if (turtle_check(AIR)) {
        return AIR;
    } else if (turtle_check(COBBLESTONE)) {
        return COBBLESTONE;
    } else if (turtle_check(GRANITE)) {
        return GRANITE;
    } else if (turtle_check(ANDESITE)) {
        return ANDESITE;
    } else if (turtle_check(DIORITE)) {
        return DIORITE;
    } else if (turtle_check(LAPIS_BLOCK)) {
        return LAPIS_BLOCK;
    } else if (turtle_check(IRON_BLOCK)) {
        return IRON_BLOCK;
    } else if (turtle_check(GOLD_BLOCK)) {
        return GOLD_BLOCK;
    } else if (turtle_check(DIAMOND_BLOCK)) {
        return DIAMOND_BLOCK;
    } else {
        return REDSTONE_BLOCK;
    }
}