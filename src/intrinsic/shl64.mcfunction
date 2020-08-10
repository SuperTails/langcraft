# l = l << s
# h = (h << s) | (l >> (32 - s))

scoreboard players operation %temp0_shl64 rust = %param0%0 rust
scoreboard players operation %temp2_shl64 rust = %param1%0 rust

scoreboard players operation %param0%0 rust = %param0%1 rust
function intrinsic:shl
scoreboard players operation %temp1_shl64 rust = %param0%0 rust

scoreboard players operation %param0%0 rust = %temp0_shl64 rust
scoreboard players set %param1%0 rust 32
scoreboard players operation %param1%0 rust -= %temp2_shl64 rust
function intrinsic:lshr
scoreboard players operation %temp1_shl64 rust += %param0%0 rust

scoreboard players operation %param0%0 rust = %temp0_shl64 rust
scoreboard players operation %param1%0 rust = %temp2_shl64 rust
function intrinsic:shl

scoreboard players operation %param0%1 rust = %temp1_shl64 rust
