# i8* dest == %param0%0
# i8* src  == %param1%0
# i32 len  == %param2%0
# i1  is_volatile == %param3%0

scoreboard players operation %%temp0_memcpy rust = %param0%0 rust
scoreboard players operation %%temp1_memcpy rust = %param1%0 rust
scoreboard players operation %%temp2_memcpy rust = %param2%0 rust

execute if score %%temp2_memcpy rust matches 1.. run function intrinsic:memcpy_inner