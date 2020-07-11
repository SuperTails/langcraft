# return <<= 1
scoreboard players operation %return%0 rust += %return%0 rust

# if param0 < 0 { c += 1 }
execute if score %param0%0 rust matches ..-1 run scoreboard players add %return%0 rust 1

# else if param0 < 0 { c += 1 }
execute if score %param1%0 rust matches ..-1 if score %param0%0 rust matches 0.. run scoreboard players add %return%0 rust 1

scoreboard players operation %param0%0 rust += %param0%0 rust
scoreboard players operation %param1%0 rust += %param1%0 rust