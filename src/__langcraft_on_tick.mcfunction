tellraw @a [{"text": "Tick occurred"}]
setblock ~ ~1 ~ minecraft:air
setblock ~ ~ ~2 minecraft:chain_command_block[facing=north]{UpdateLastExecution:0b,auto:1b}
clone ~ ~1 ~1 ~ ~1 ~1 ~ ~ ~1
tp @e[tag=next] ~ ~ ~2