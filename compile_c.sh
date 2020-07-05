clang -v -g -O2 -m32 -c -emit-llvm mcfunction.c -o mcfunction.bc
clang -v -g -O2 -m32 -S -emit-llvm mcfunction.c -o mcfunction.ll