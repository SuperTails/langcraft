# arguments:
# %ptr - The location to write to
# %param2%0 - The byte to write

function intrinsic:setptr

execute at @e[tag=ptr] store result score %param0%0 rust run data get block ~ ~ ~ RecordItem.tag.Memory 1

scoreboard players operation %%temp0_store_byte rust = %ptr rust
scoreboard players operation %%temp0_store_byte rust %= %%FOUR rust

# 0xFFFF_FF00
execute if score %%temp0_store_byte rust matches 0..0 run scoreboard players set %param1%0 rust -256
# 0xFFFF_00FF
execute if score %%temp0_store_byte rust matches 1..1 run scoreboard players set %param1%0 rust -65281
# 0xFF00_FFFF
execute if score %%temp0_store_byte rust matches 2..2 run scoreboard players set %param1%0 rust -16711681
# 0x00FF_FFFF
execute if score %%temp0_store_byte rust matches 3..3 run scoreboard players set %param1%0 rust 16777215

function intrinsic:and

# %param2%0 *= 1 << 0
execute if score %%temp0_store_byte rust matches 0..0 run scoreboard players operation %param2%0 rust *= %%1 rust
# %param2%0 *= 1 << 8
execute if score %%temp0_store_byte rust matches 1..1 run scoreboard players operation %param2%0 rust *= %%256 rust
# %param2%0 *= 1 << 16
execute if score %%temp0_store_byte rust matches 2..2 run scoreboard players operation %param2%0 rust *= %%65536 rust
# %param2%0 *= 1 << 24
execute if score %%temp0_store_byte rust matches 3..3 run scoreboard players operation %param2%0 rust *= %%16777216 rust

scoreboard players operation %return%0 rust += %param2%0 rust

execute at @e[tag=ptr] store result block ~ ~ ~ RecordItem.tag.Memory int 1 run scoreboard players get %return%0 rust
