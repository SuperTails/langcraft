scoreboard players operation %ptr rust = %%temp1_memcpy rust

function intrinsic:load_byte
scoreboard players operation %%temp4_memcpy rust = %param0%0 rust

scoreboard players add %ptr rust 1
function intrinsic:load_byte
scoreboard players operation %param0%0 rust *= %%256 rust
scoreboard players operation %%temp4_memcpy rust += %param0%0 rust

scoreboard players add %ptr rust 1
function intrinsic:load_byte
scoreboard players operation %param0%0 rust *= %%65536 rust
scoreboard players operation %%temp4_memcpy rust += %param0%0 rust

scoreboard players add %ptr rust 1
function intrinsic:load_byte
scoreboard players operation %param0%0 rust *= %%16777216 rust
scoreboard players operation %%temp4_memcpy rust += %param0%0 rust

scoreboard players operation %ptr rust = %%temp0_memcpy rust
function intrinsic:setptr
execute at @e[tag=ptr] store result block ~ ~ ~ RecordItem.tag.Memory int 1 run scoreboard players get %%temp4_memcpy rust

# src += 4
scoreboard players add %%temp1_memcpy rust 4
# dest += 4
scoreboard players add %%temp0_memcpy rust 4
# len -= 4
scoreboard players remove %%temp2_memcpy rust 4

execute if score %%temp2_memcpy rust matches 4.. run function intrinsic:memcpy/inner