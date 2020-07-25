clang -v -g -m32 -O2 -c -emit-llvm mcfunction.c -o mcfunction.bc
clang -v -g -m32 -O2 -S -emit-llvm mcfunction.c -o mcfunction.ll