# temp = *src
scoreboard players operation %ptr rust = %%temp1_memcpy rust
function intrinsic:load_byte

# *dest = temp
scoreboard players operation %param2%0 rust = %param0%0 rust
scoreboard players operation %ptr rust = %%temp0_memcpy rust
function intrinsic:store_byte

# src += 1
scoreboard players add %%temp1_memcpy rust 1
# dest += 1
scoreboard players add %%temp0_memcpy rust 1
# len -= 1
scoreboard players remove %%temp2_memcpy rust 1

scoreboard players add %%tempbytes_memcpy rust 1