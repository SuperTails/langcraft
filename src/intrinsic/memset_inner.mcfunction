function intrinsic:setptr
execute at @e[tag=ptr] store result block ~ ~ ~ RecordItem.tag.Memory int 1 run scoreboard players get %%temp4_memset rust

scoreboard players add %ptr rust 4
scoreboard players remove %%temp2_memset rust 4

execute if score %%temp2_memset rust matches 4.. run function intrinsic:memset_inner