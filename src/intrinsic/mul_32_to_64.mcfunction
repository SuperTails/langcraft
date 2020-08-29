# Arguments:
# %param0%0 - Left operand
# %param1%0 - Right operand
# %return%0 - Low word of product
# %return%1 - High word of product

scoreboard players operation %%tempmul_p0_save rust = %param0%0 rust
scoreboard players operation %%tempmul_p1_save rust = %param1%0 rust

scoreboard players operation %%tempmul_%0%0 rust = %param0%0 rust
scoreboard players operation %%tempmul_%1%0 rust = %param1%0 rust

scoreboard players operation %%tempmul_%5%0 rust = %%tempmul_%0%0 rust
scoreboard players operation %%tempmul_%5%0 rust %= %%65536 rust

scoreboard players operation %%tempmul_%6%0 rust = %%tempmul_%1%0 rust
scoreboard players operation %%tempmul_%6%0 rust %= %%65536 rust

scoreboard players operation %%tempmul_%7%0 rust = %%tempmul_%6%0 rust
scoreboard players operation %%tempmul_%7%0 rust *= %%tempmul_%5%0 rust

scoreboard players operation %%tempmul_%8%0 rust = %%tempmul_%7%0 rust
scoreboard players operation %%tempmul_%8%0 rust %= %%65536 rust

scoreboard players set %%tempmul_%temp3 rust 16
scoreboard players operation %param0%0 rust = %%tempmul_%7%0 rust
scoreboard players operation %param1%0 rust = %%tempmul_%temp3 rust
function intrinsic:lshr
scoreboard players operation %%tempmul_%9%0 rust = %param0%0 rust
scoreboard players set %%tempmul_%temp4 rust 16
scoreboard players operation %param0%0 rust = %%tempmul_%0%0 rust
scoreboard players operation %param1%0 rust = %%tempmul_%temp4 rust
function intrinsic:lshr
scoreboard players operation %%tempmul_%10%0 rust = %param0%0 rust
scoreboard players operation %%tempmul_%11%0 rust = %%tempmul_%6%0 rust
scoreboard players operation %%tempmul_%11%0 rust *= %%tempmul_%10%0 rust
scoreboard players operation %%tempmul_%12%0 rust = %%tempmul_%9%0 rust
scoreboard players operation %%tempmul_%12%0 rust += %%tempmul_%11%0 rust

scoreboard players operation %%tempmul_%13%0 rust = %%tempmul_%12%0 rust
scoreboard players operation %%tempmul_%13%0 rust %= %%65536 rust

scoreboard players set %%tempmul_%temp6 rust 16
scoreboard players operation %param0%0 rust = %%tempmul_%12%0 rust
scoreboard players operation %param1%0 rust = %%tempmul_%temp6 rust
function intrinsic:lshr
scoreboard players operation %%tempmul_%14%0 rust = %param0%0 rust
scoreboard players set %%tempmul_%temp7 rust 16
scoreboard players operation %param0%0 rust = %%tempmul_%1%0 rust
scoreboard players operation %param1%0 rust = %%tempmul_%temp7 rust
function intrinsic:lshr
scoreboard players operation %%tempmul_%15%0 rust = %param0%0 rust
scoreboard players operation %%tempmul_%16%0 rust = %%tempmul_%15%0 rust
scoreboard players operation %%tempmul_%16%0 rust *= %%tempmul_%5%0 rust
scoreboard players operation %%tempmul_%17%0 rust = %%tempmul_%13%0 rust
scoreboard players operation %%tempmul_%17%0 rust += %%tempmul_%16%0 rust
scoreboard players set %%tempmul_%temp8 rust 16
scoreboard players operation %param0%0 rust = %%tempmul_%17%0 rust
scoreboard players operation %param1%0 rust = %%tempmul_%temp8 rust
function intrinsic:lshr
scoreboard players operation %%tempmul_%18%0 rust = %param0%0 rust

scoreboard players operation %%tempmul_%19%0 rust = %%tempmul_%15%0 rust
scoreboard players operation %%tempmul_%19%0 rust *= %%tempmul_%10%0 rust
scoreboard players operation %%tempmul_%20%0 rust = %%tempmul_%14%0 rust
scoreboard players operation %%tempmul_%20%0 rust += %%tempmul_%19%0 rust
scoreboard players operation %%tempmul_%21%0 rust = %%tempmul_%17%0 rust
scoreboard players operation %%tempmul_%21%0 rust *= %%65536 rust
scoreboard players operation %%tempmul_%22%0 rust = %%tempmul_%20%0 rust
scoreboard players operation %%tempmul_%22%0 rust += %%tempmul_%18%0 rust


scoreboard players operation %param0%0 rust = %%tempmul_%21%0 rust
scoreboard players operation %param1%0 rust = %%tempmul_%8%0 rust
function intrinsic:or
scoreboard players operation %%tempmul_%23%0 rust = %return%0 rust

scoreboard players operation %return%0 rust = %%tempmul_%23%0 rust
scoreboard players operation %return%1 rust = %%tempmul_%22%0 rust

scoreboard players operation %param0%0 rust = %%tempmul_p0_save rust
scoreboard players operation %param1%0 rust = %%tempmul_p1_save rust