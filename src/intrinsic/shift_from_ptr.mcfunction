scoreboard players operation %%ptr rust = %ptr rust
scoreboard players operation %%ptr rust %= %%4 rust

# let %param0%1 = %param0%0 < 0
execute store success score %param0%1 rust if score %param0%0 rust matches ..-1

execute if score %param0%1 rust matches 1..1 run scoreboard players operation %param0%0 rust *= %%-1 rust
execute if score %param0%1 rust matches 1..1 run scoreboard players operation %param0%0 rust += %%-1 rust

execute if score %%ptr rust matches 1.. run function intrinsic:shift_from_ptr_inner

execute if score %param0%1 rust matches 1..1 run scoreboard players operation %param0%0 rust *= %%-1 rust
execute if score %param0%1 rust matches 1..1 run scoreboard players operation %param0%0 rust += %%-1 rust

scoreboard players operation %%ptr rust = %ptr rust
scoreboard players operation %%ptr rust %= %%4 rust

execute if score %param0%1 rust matches 1..1 if score %%ptr rust matches 1..1 run scoreboard players add %param0%0 rust 16777216
execute if score %param0%1 rust matches 1..1 if score %%ptr rust matches 2..2 run scoreboard players add %param0%0 rust 65536 
execute if score %param0%1 rust matches 1..1 if score %%ptr rust matches 3..3 run scoreboard players add %param0%0 rust 256