scoreboard players operation %%tempfshr_%0%0 rust = %param0%0 rust
scoreboard players operation %%tempfshr_%1%0 rust = %param1%0 rust
scoreboard players operation %%tempfshr_%2%0 rust = %param2%0 rust

scoreboard players operation %%tempfshr_%4%0 rust = %%tempfshr_%2%0 rust
scoreboard players operation %%tempfshr_%4%0 rust %= %%32 rust

execute store success score %%tempfshr_%5%0 rust if score %%tempfshr_%4%0 rust matches 0..0

scoreboard players set %%tempfshr_%6%0 rust 32
scoreboard players operation %%tempfshr_%6%0 rust -= %%tempfshr_%4%0 rust

scoreboard players operation %param0%0 rust = %%tempfshr_%0%0 rust
scoreboard players operation %param1%0 rust = %%tempfshr_%6%0 rust
function intrinsic:shl
scoreboard players operation %%tempfshr_%7%0 rust = %param0%0 rust

scoreboard players operation %param0%0 rust = %%tempfshr_%1%0 rust
scoreboard players operation %param1%0 rust = %%tempfshr_%4%0 rust
function intrinsic:lshr
scoreboard players operation %%tempfshr_%8%0 rust = %param0%0 rust

scoreboard players operation %%tempfshr_%9%0 rust = %%tempfshr_%7%0 rust
scoreboard players operation %%tempfshr_%9%0 rust += %%tempfshr_%8%0 rust
execute if score %%tempfshr_%5%0 rust matches 1..1 run scoreboard players operation %%tempfshr_%10%0 rust = %%tempfshr_%1%0 rust
execute unless score %%tempfshr_%5%0 rust matches 1..1 run scoreboard players operation %%tempfshr_%10%0 rust = %%tempfshr_%9%0 rust

scoreboard players operation %return%0 rust = %%tempfshr_%10%0 rust