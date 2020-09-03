# i8* dest == %param0%0
# i8 value == %param1%0
# i32 len  == %param2%0
# i1 is_volatile == %param3%0

# !INTERPRETER: ASSERT if score %param2%0 rust matches 0..

scoreboard players operation %ptr rust = %param0%0 rust
scoreboard players operation %%temp1_memset rust = %param1%0 rust
scoreboard players operation %%temp2_memset rust = %param2%0 rust

# start_bytes = dest % 4
scoreboard players operation %%temp3_memset rust = %ptr rust
scoreboard players operation %%temp3_memset rust %= %%4 rust

# if len > 0 && start_bytes == 3 {
#   *dest = value;
#   dest += 1;
#   start_bytes -= 1;
# }
scoreboard players operation %param2%0 rust = %%temp1_memset rust
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 1..1 run function intrinsic:store_byte
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 1..1 run scoreboard players add %ptr rust 1
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 1..1 run scoreboard players remove %%temp2_memset rust 1
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 1..1 run scoreboard players add %%temp3_memset rust 1

scoreboard players operation %param2%0 rust = %%temp1_memset rust
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 2..2 run function intrinsic:store_byte
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 2..2 run scoreboard players add %ptr rust 1
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 2..2 run scoreboard players remove %%temp2_memset rust 1
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 2..2 run scoreboard players add %%temp3_memset rust 1

scoreboard players operation %param2%0 rust = %%temp1_memset rust
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 3..3 run function intrinsic:store_byte
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 3..3 run scoreboard players add %ptr rust 1
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 3..3 run scoreboard players remove %%temp2_memset rust 1
execute if score %%temp2_memset rust matches 1.. if score %%temp3_memset rust matches 3..3 run scoreboard players add %%temp3_memset rust 1

scoreboard players operation %%temp4_memset rust = %%temp1_memset rust
scoreboard players operation %%temp4_memset rust *= %%256 rust
scoreboard players operation %%temp4_memset rust += %%temp1_memset rust
scoreboard players operation %%temp4_memset rust *= %%256 rust
scoreboard players operation %%temp4_memset rust += %%temp1_memset rust
scoreboard players operation %%temp4_memset rust *= %%256 rust
scoreboard players operation %%temp4_memset rust += %%temp1_memset rust

execute if score %%temp2_memset rust matches 4.. run function intrinsic:memset_inner

scoreboard players operation %param2%0 rust = %%temp1_memset rust
execute if score %%temp2_memset rust matches 1.. run function intrinsic:store_byte
execute if score %%temp2_memset rust matches 1.. run scoreboard players add %ptr rust 1
execute if score %%temp2_memset rust matches 1.. run scoreboard players remove %%temp2_memset rust 1

scoreboard players operation %param2%0 rust = %%temp1_memset rust
execute if score %%temp2_memset rust matches 1.. run function intrinsic:store_byte
execute if score %%temp2_memset rust matches 1.. run scoreboard players add %ptr rust 1
execute if score %%temp2_memset rust matches 1.. run scoreboard players remove %%temp2_memset rust 1

scoreboard players operation %param2%0 rust = %%temp1_memset rust
execute if score %%temp2_memset rust matches 1.. run function intrinsic:store_byte