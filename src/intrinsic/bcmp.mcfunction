# Arguments:
# %param0%0 - pointer s1
# %param1%0 - pointer s2
# %param2%0 - integer len

scoreboard players operation %%temp0_bcmp rust = %param0%0 rust
scoreboard players operation %%temp1_bcmp rust = %param1%0 rust
scoreboard players operation %%temp2_bcmp rust = %param2%0 rust

scoreboard players set %return%0 rust 0

execute if score %%temp2_bcmp rust matches 1.. run function intrinsic:bcmp_inner