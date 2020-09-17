# This code was derived from <top>/src/intrinsics/lshr/inner.mcfunction.

function intrinsic:lshr/getshift
scoreboard players remove %%temp1_lshr_inner rust 1
execute if score %param0%0 rust matches ..-1 run scoreboard players operation %param0%0 rust -= %%temp1_lshr_inner rust
scoreboard players add %%temp1_lshr_inner rust 1
scoreboard players operation %param0%0 rust /= %%temp1_lshr_inner rust
