rustc rustfunction.rs -C panic=abort --emit=llvm-ir,llvm-bc --crate-type=rlib --target=i686-unknown-linux-gnu
