## Langcraft
### The LLVM target for Minecraft you've never wanted

Langcraft is a code generator targeting [Minecraft Data Packs](https://minecraft.gamepedia.com/Data_Pack). It can currently run a fairly substantial set of bitcode files without issue. The project has a built-in command interpreter for debugging that supports breakpoints (ish) and inspecting register/memory values. All generated datapacks can be run in a real Minecraft Java Edition 1.16+ world in under 5 minutes.

### Usage
```
cargo run -- --arg1 --arg2 ./path/to/llvm/bitcode.bc
```
All arguments must come before the path. Valid arguments are:
 - `--out=path/to/dir/`: Specify the directory the datapack files should be placed in (default is `./out`)
 - `--run`: Run the command interpreter on the generated code

To use the generated datapack in Minecraft:
 1. Copy the entire output folder (`./out` by default) to the `datapacks/` directory of a Minecraft world (using a superflat void world is recommended)
 2. Run `/function setup:setup`. This only has to be done the first time a Langcraft datapack is used in a world.
 3. Run `/function rust:run`
 4. If the datapack is modified while the world is open, run `/reload` and then go back to step 3.

Rust code must be built as follows:
 - Release mode 
 - `panic=abort`
 - `#![no_std]`
 - `#![no_main]`
 - Have a `main` function with `#[no_mangle]`
 - Use `i686-unknown-linux`

`rust_interp` is a Rust project already configured to generate the proper bitcode. The `interpreter` binary target as shown in the demo can be built with:
```
sh compile_rust.sh
```

And the file to use will be:

`rust_interp/target/i686-unknown-linux-gnu/release/deps/interpreter-SOMEHEXSTRING.bc`

Any other language capable of generating LLVM bitcode can be used, as long as it can be built for a bare-metal 32-bit target. For a clang example see `compile_c.sh`.

### Demo
A video of a Langcraft-compiled interpreter can be seen [here](https://youtu.be/Cx0w5Wn9pPU).