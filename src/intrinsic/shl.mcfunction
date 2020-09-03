# Arguments:
# %param0%0 - The value to be shifted (also the output)
# %param1%0 - The amount to shift by, is clobbered

execute if score %param1%0 rust matches 1.. run scoreboard players operation %param0%0 rust *= %%2 rust
scoreboard players remove %param1%0 rust 1
execute if score %param1%0 rust matches 0.. run function intrinsic:shl