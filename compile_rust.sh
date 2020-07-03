rustc rustfunction.rs -C panic=abort --emit=llvm-ir --crate-type=rlib --target=i686-unknown-linux-gnu
