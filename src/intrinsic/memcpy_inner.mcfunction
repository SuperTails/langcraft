# temp = *src
scoreboard players operation %ptr rust = %%temp1_memcpy rust
function intrinsic:load_byte


# *dest = temp
scoreboard players operation %param2%0 rust = %param0%0 rust
scoreboard players operation %ptr rust = %%temp0_memcpy rust
function intrinsic:store_byte

# ++dest
scoreboard players add %%temp0_memcpy rust 1
# ++src
scoreboard players add %%temp1_memcpy rust 1
# --len
scoreboard players remove %%temp2_memcpy rust 1

# FIXME: This will also break on a longer memcpy
execute if score %%temp2_memcpy rust matches 1.. run function intrinsic:memcpy_inner