scoreboard players operation %ptr rust = %%temp0_bcmp rust
function intrinsic:load_byte
scoreboard players operation %%temp3_bcmp rust = %param0%0 rust

scoreboard players operation %ptr rust = %%temp1_bcmp rust
function intrinsic:load_byte
scoreboard players operation %%temp4_bcmp rust = %param0%0 rust

scoreboard players add %%temp0_bcmp rust 1
scoreboard players add %%temp1_bcmp rust 1
scoreboard players remove %%temp2_bcmp rust 1

execute unless score %%temp3_bcmp rust = %%temp4_bcmp rust run scoreboard players set %return%0 rust 1
execute if score %%temp2_bcmp rust matches 1.. run function intrinsic:bcmp_inner
