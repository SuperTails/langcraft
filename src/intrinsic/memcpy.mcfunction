# i8* dest == %param0%0
# i8* src  == %param1%0
# i32 len  == %param2%0
# i1  is_volatile == %param3%0

scoreboard players operation %temp1a rust = %param2%0 rust
scoreboard players operation %temp1a rust %= %%FOUR rust
# !INTERPRETER: ASSERT if score %temp1a rust matches 0..0

# FIXME: This will break once we support smaller memcpy lengths
scoreboard players operation %param2%0 rust /= %%FOUR rust

function intrinsic:memcpy_inner