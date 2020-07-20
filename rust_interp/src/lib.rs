#![feature(rustc_attrs)]
#![no_std]

#[repr(i32)]
#[derive(/*Debug,*/ PartialEq, PartialOrd, Clone, Copy)]
pub enum McBlock {
    Air,
    Cobblestone,
    Granite,
    Andesite,
    Diorite,
    LapisBlock,
    IronBlock,
    GoldBlock,
    DiamondBlock,
    RedstoneBlock,
}

static MC_BLOCKS: [McBlock; 10] = [
    McBlock::Air,
    McBlock::Cobblestone,
    McBlock::Granite,
    McBlock::Andesite,
    McBlock::Diorite,
    McBlock::LapisBlock,
    McBlock::IronBlock,
    McBlock::GoldBlock,
    McBlock::DiamondBlock,
    McBlock::RedstoneBlock,
];

/*impl core::fmt::Display for McBlock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "minecraft:")?;

        match self {
            McBlock::Air => write!(f, "air"),
            McBlock::Cobblestone => write!(f, "cobblestone"),
            McBlock::Granite => write!(f, "granite"),
            McBlock::Andesite => write!(f, "andesite"),
            McBlock::Diorite => write!(f, "diorite"),
            McBlock::LapisBlock => write!(f, "lapis_block"),
            McBlock::IronBlock => write!(f, "iron_block"),
            McBlock::GoldBlock => write!(f, "gold_block"),
            McBlock::DiamondBlock => write!(f, "diamond_block"),
            McBlock::RedstoneBlock => write!(f, "redstone_block"),
        }
    }
}*/

use core::panic::PanicInfo;

extern "C" {
    #[rustc_args_required_const(0, 1)]
    pub fn print_raw(data: *const u8, len: usize);
    pub fn print(value: i32);
    pub fn init();

    pub fn turtle_x(value: i32);
    pub fn turtle_y(value: i32);
    pub fn turtle_z(value: i32);

    /// Sets the block at the turtle's position
    pub fn turtle_set(block: McBlock);

    /// Returns 1 if the block at the turtle's position matches the argument
    pub fn turtle_check(block: McBlock) -> bool;

    /// Returns the block at the turtle's position
    pub fn turtle_get() -> McBlock;

    /// Returns the char at the turtle's position
    pub fn turtle_get_char() -> u8;
}

#[macro_export]
macro_rules! print_str {
    ($data:expr) => {
        print_raw($data.as_ptr(), $data.len())
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { print_str!(b"Panic") };
    loop {}
}