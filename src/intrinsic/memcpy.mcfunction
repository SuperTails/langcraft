# i8* dest == %param0%0
# i8* src  == %param1%0
# i32 len  == %param2%0
# i1  is_volatile == %param3%0

execute if score %param4%0 rust matches 1.. run setblock ~ ~1 ~ minecraft:air

scoreboard players operation %%temp0_memcpy rust = %param0%0 rust
scoreboard players operation %%temp1_memcpy rust = %param1%0 rust
scoreboard players operation %%temp2_memcpy rust = %param2%0 rust

scoreboard players operation %%temptotal_memcpy rust = %%temp2_memcpy rust
scoreboard players operation %%temp2_memcpy rust < %%1024 rust
scoreboard players operation %%temptotal_memcpy rust -= %%temp2_memcpy rust

scoreboard players operation %%tempbytes_memcpy rust = %%temp0_memcpy rust
scoreboard players operation %%tempbytes_memcpy rust %= %%4 rust

execute if score %%temp2_memcpy rust matches 1.. if score %%tempbytes_memcpy rust matches 1..1 run function intrinsic:memcpy/next_byte
execute if score %%temp2_memcpy rust matches 1.. if score %%tempbytes_memcpy rust matches 2..2 run function intrinsic:memcpy/next_byte
execute if score %%temp2_memcpy rust matches 1.. if score %%tempbytes_memcpy rust matches 3..3 run function intrinsic:memcpy/next_byte

execute if score %%temp2_memcpy rust matches 4.. run function intrinsic:memcpy/inner

execute if score %%temp2_memcpy rust matches 1.. run function intrinsic:memcpy/next_byte
execute if score %%temp2_memcpy rust matches 1.. run function intrinsic:memcpy/next_byte
execute if score %%temp2_memcpy rust matches 1.. run function intrinsic:memcpy/next_byte

scoreboard players operation %param0%0 rust = %%temp0_memcpy rust
scoreboard players operation %param1%0 rust = %%temp1_memcpy rust
scoreboard players operation %param2%0 rust = %%temptotal_memcpy rust
execute if score %param2%0 rust matches 1.. if score %param4%0 rust matches 1..1 run setblock ~ ~1 ~ minecraft:redstone_block destroy
execute if score %param2%0 rust matches 0..0 if score %param4%0 rust matches 1..1 run function intrinsic:pop_and_branch