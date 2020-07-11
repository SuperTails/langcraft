scoreboard players operation %param0%0 rust /= %%256 rust
scoreboard players remove %%ptr rust 1
execute if score %%ptr rust matches 1.. run function intrinsic:shift_from_ptr_inner