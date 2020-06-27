clang -c -emit-llvm mcfunction.c -o mcfunction.bc
clang -S -emit-llvm mcfunction.c -o mcfunction.ll