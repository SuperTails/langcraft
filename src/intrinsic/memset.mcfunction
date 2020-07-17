# i8* dest == %param0%0
# i8 value == %param1%0
# i32 len  == %param2%0
# i1 is_volatile == %param3%0

# !INTERPRETER: ASSERT if score %param2%0 rust matches 0..

scoreboard players operation %%temp0_memset rust = %param0%0 rust
scoreboard players operation %%temp1_memset rust = %param1%0 rust
scoreboard players operation %%temp2_memset rust = %param2%0 rust

execute if score %%temp2_memset rust matches 1.. run function intrinsic:memset_inner