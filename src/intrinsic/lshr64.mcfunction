# h = h >> s
# l = (l >> s) | (h << (32 - s))

scoreboard players operation %temp2_lshr64 rust = %param1%0 rust

execute if score %param1%0 rust matches 32.. run scoreboard players set %%temp1_lshr_inner rust -2147483648
execute if score %param1%0 rust matches 0..31 run function intrinsic:lshr
execute if score %param1%0 rust matches 32.. run scoreboard players set %param0%0 rust 0
scoreboard players operation %temp1_lshr64 rust = %param0%0 rust

scoreboard players operation %param0%0 rust = %param0%1 rust
scoreboard players set %param1%0 rust 32
scoreboard players operation %param1%0 rust -= %temp2_lshr64 rust
function intrinsic:shl
scoreboard players operation %temp1_lshr64 rust += %param0%0 rust

scoreboard players operation %param0%0 rust = %param0%1 rust
scoreboard players operation %param1%0 rust = %temp2_lshr64 rust
function intrinsic:lshr

scoreboard players operation %param0%1 rust = %param0%0 rust
scoreboard players operation %param0%0 rust = %temp1_lshr64 rust
