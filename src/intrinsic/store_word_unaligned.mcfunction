# Arguments
# %ptr      - Address to store at
# %param0%0 - Word to be stored

scoreboard players operation %%temp0_swu rust = %param0%0 rust

# FIXME: This may not actually work like an `and`
scoreboard players operation %param2%0 rust = %%temp0_swu rust
scoreboard players operation %param2%0 rust %= %%256 rust
function intrinsic:store_byte
scoreboard players add %ptr rust 1

scoreboard players operation %param0%0 rust = %%temp0_swu rust
scoreboard players set %param1%0 rust 8
function intrinsic:lshr
scoreboard players operation %param2%0 rust = %param0%0 rust
scoreboard players operation %param2%0 rust %= %%256 rust
function intrinsic:store_byte
scoreboard players add %ptr rust 1

scoreboard players operation %param0%0 rust = %%temp0_swu rust
scoreboard players set %param1%0 rust 16
function intrinsic:lshr
scoreboard players operation %param2%0 rust = %param0%0 rust
scoreboard players operation %param2%0 rust %= %%256 rust
function intrinsic:store_byte
scoreboard players add %ptr rust 1

scoreboard players operation %param0%0 rust = %%temp0_swu rust
scoreboard players set %param1%0 rust 24
function intrinsic:lshr
scoreboard players operation %param2%0 rust = %param0%0 rust
scoreboard players operation %param2%0 rust %= %%256 rust
function intrinsic:store_byte