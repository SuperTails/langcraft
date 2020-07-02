clang -v -m32 -c -emit-llvm mcfunction.c -o mcfunction.bc
clang -v -m32 -S -emit-llvm mcfunction.c -o mcfunction.ll