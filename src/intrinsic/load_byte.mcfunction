# %ptr - The location to read from
# %param0%0 - The return value
# Clobbers %param1%0

function intrinsic:setptr

execute at @e[tag=ptr] store result score %param0%0 rust run data get block ~ ~ ~ RecordItem.tag.Memory 1

scoreboard players operation %param1%0 rust = %ptr rust
scoreboard players operation %param1%0 rust %= %%FOUR rust
# 1 << (8 * 3)
execute if score %param1%0 rust matches 0..0 run scoreboard players set %param1%0 rust 16777216
# 1 << (8 * 2)
execute if score %param1%0 rust matches 1..1 run scoreboard players set %param1%0 rust 65536
# 1 << (8 * 1)
execute if score %param1%0 rust matches 2..2 run scoreboard players set %param1%0 rust 256
# 1 << (8 * 0)
execute if score %param1%0 rust matches 3..3 run scoreboard players set %param1%0 rust 1

scoreboard players operation %param0%0 rust *= %param1%0 rust

scoreboard players set %param1%0 rust 24

function intrinsic:lshr