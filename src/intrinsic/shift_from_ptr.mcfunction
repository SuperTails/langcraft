scoreboard players operation %%ptr rust = %ptr rust
scoreboard players operation %%ptr rust %= %%FOUR rust
execute if score %%ptr rust matches 1.. run function intrinsic:shift_from_ptr_inner
