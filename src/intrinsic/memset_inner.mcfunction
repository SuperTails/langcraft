scoreboard players operation %ptr rust = %%temp0_memset rust
scoreboard players operation %param2%0 rust = %%temp1_memset rust
function intrinsic:store_byte

scoreboard players add %%temp0_memset rust 1
scoreboard players remove %%temp2_memset rust 1

execute if score %%temp2_memset rust matches 1.. run function intrinsic:memset_inner