tellraw @a [{"text": "%stackptr at start of pop_and_branch is "}, {"score": {"name": "%stackptr", "objective": "rust" } }]
scoreboard players remove %stackptr rust 4
scoreboard players operation %ptr rust = %stackptr rust
function intrinsic:setptr
execute as @e[tag=ptr] at @s store result entity @s Pos[2] double 1 run data get block ~ ~ ~ RecordItem.tag.Memory 1
execute as @e[tag=ptr] at @s run tp @s -2 1 ~
execute at @e[tag=ptr] run setblock ~ ~ ~ minecraft:redstone_block
