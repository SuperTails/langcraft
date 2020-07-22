# tellraw @a [{"text": "%stackptr at start of pop_and_branch is "}, {"score": {"name": "%stackptr", "objective": "rust" } }]
scoreboard players remove %stackptr rust 4
scoreboard players operation %ptr rust = %stackptr rust
function intrinsic:setptr
execute at @e[tag=ptr] store result score %%temp0_pab rust run data get block ~ ~ ~ RecordItem.tag.Memory 1

scoreboard players operation %%tempz_pab rust = %%temp0_pab rust
# scoreboard players operation %%tempz_pab rust %= %%32 rust

scoreboard players set %%tempx_pab rust 0
# scoreboard players operation %%tempx_pab rust = %%temp0_pab rust
# scoreboard players operation %%tempx_pab rust /= %%32 rust
# scoreboard players operation %%tempx_pab rust *= %%-1 rust

execute as @e[tag=ptr] store result entity @s Pos[0] double 1 run scoreboard players get %%tempx_pab rust
execute as @e[tag=ptr] store result entity @s Pos[2] double 1 run scoreboard players get %%tempz_pab rust
execute as @e[tag=ptr] at @s run tp @s ~-2 1 ~
execute at @e[tag=ptr] run setblock ~ ~ ~ minecraft:redstone_block