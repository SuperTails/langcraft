## Langcraft
### The LLVM target for Minecraft you've never wanted

Langcraft is a code generator targeting [Minecraft Data Packs](https://minecraft.gamepedia.com/Data_Pack). It can currently run a limited set of bitcode files (usually C works great, Rust is more limited sometimes).
Currently Rust programs must be built in `--release` so that strings are inlined into print commands. The project also has a built-in command interpreter, which currently is the only way to run larger programs.
Support for automatically generating the necessary blocks and entities in a real Minecraft world is coming.

### Usage
To compile and then interpret a bitcode file, run:
```
cargo run -- ./path/to/llvm/bitcode.bc
```

`rust_interp` is a Rust project already configured to generate the proper bitcode. It can be built with:
```
sh compile_rust.sh
```
And the file to use will be:

`rust_interp/target/i686-unknown-linux-gnu/release/deps/parser-SOMEHEXSTRING.bc`
``