scoreboard players operation %param0%0 rust *= %%2 rust
scoreboard players add %return%0 rust 1

execute if score %return%0 rust matches ..31 if score %param0%0 rust matches 0.. run function intrinsic:llvm_ctlz_i32_inner