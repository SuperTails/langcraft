# Code (from stack overflow post that I need to link in a moment)
# // logical shift right (unsigned)
# if (shift > 15) {
#     a = 0; // more than 15, becomes zero
# } else if (shift > 0) {
#     if (a < 0) {
#         // deal with the sign bit (15)
#         a += -32768;
#         a /= powtab[shift];
#         a += powtab[15 - shift];
#     } else {
#         a /= powtab[shift];
#     }
# }
#
# Pseudo-datapack code (for 32-bit integers)
#
# lshr:
# if (shift > 31) { a = 0 }
# if (shift > 0) { call inner }
#
# inner:
# pow = powtab[shift] # but how to calculate this??
# cond = a < 0
# if (cond) { a += i32::MIN }
# a /= pow
# if (cond) { a += powtab[15 - shift] }

# %param0%0 : a (mutated, also the return value)
# %param1%0 : shift (clobbered)

execute if score %param1%0 rust matches 32.. run scoreboard players set %param0%0 rust 0
execute if score %param1%0 rust matches 1.. run function intrinsic:lshr/inner