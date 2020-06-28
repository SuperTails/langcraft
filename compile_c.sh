clang -m32 -c -emit-llvm mcfunction.c -o mcfunction.bc
clang -m32 -S -emit-llvm mcfunction.c -o mcfunction.ll