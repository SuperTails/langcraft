# Arguments
# %ptr
# Return value is %return%0

function intrinsic:load_byte
scoreboard players operation %return%0 rust = %param0%0 rust
scoreboard players add %ptr rust 1

function intrinsic:load_byte
scoreboard players operation %param0%0 rust *= %%256 rust
scoreboard players operation %return%0 rust += %param0%0 rust
scoreboard players add %ptr rust 1

function intrinsic:load_byte
scoreboard players operation %param0%0 rust *= %%65536 rust
scoreboard players operation %return%0 rust += %param0%0 rust
scoreboard players add %ptr rust 1

function intrinsic:load_byte
scoreboard players operation %param0%0 rust *= %%16777216 rust
scoreboard players operation %return%0 rust += %param0%0 rust