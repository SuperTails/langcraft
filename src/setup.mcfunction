fill 0 0 0 127 15 15 minecraft:air

# Build row
setblock 0 0 0 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 1 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 2 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 3 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 4 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 5 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 6 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 7 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 8 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 9 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 10 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 11 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 12 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 13 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 14 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}
setblock 0 0 15 minecraft:jukebox{RecordItem:{id:"minecraft:stone",Count:1b,tag:{Memory:1}}}

# Build plane
clone 0 0 0 0 0 15 0 1 0
clone 0 0 0 0 1 15 0 2 0
clone 0 0 0 0 3 15 0 4 0
clone 0 0 0 0 7 15 0 8 0

# Build cubes
clone 0 0 0 0 15 15 1 0 0
clone 0 0 0 1 15 15 2 0 0
clone 0 0 0 3 15 15 4 0 0
clone 0 0 0 7 15 15 8 0 0
clone 0 0 0 15 15 15 16 0 0
clone 0 0 0 31 15 15 32 0 0
clone 0 0 0 63 15 15 64 0 0 

scoreboard objectives remove rust
scoreboard objectives add rust dummy

kill @e[tag=ptr]
kill @e[tag=turtle]

data modify storage langcraft:stdout chars set value []

summon minecraft:armor_stand 0 0 0 {Marker:1b,Tags:["ptr"]}
summon minecraft:armor_stand 0 0 0 {Marker:1b,Tags:["turtle"]}