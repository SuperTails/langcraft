rustc rustfunction.rs --verbose -C panic=abort -C opt-level=2 --emit=llvm-ir,llvm-bc --crate-type=rlib --target=i686-unknown-linux-gnu
