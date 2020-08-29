; ModuleID = 'mcfunction.c'
source_filename = "mcfunction.c"
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i386-pc-linux-gnu"

; Function Attrs: noinline nounwind optnone
define dso_local void @print_result(i32 (i32)* %0) #0 !dbg !8 {
  %2 = alloca i32 (i32)*, align 4
  store i32 (i32)* %0, i32 (i32)** %2, align 4
  call void @llvm.dbg.declare(metadata i32 (i32)** %2, metadata !15, metadata !DIExpression()), !dbg !16
  %3 = load i32 (i32)*, i32 (i32)** %2, align 4, !dbg !17
  %4 = call i32 %3(i32 41), !dbg !17
  call void @print(i32 %4), !dbg !18
  ret void, !dbg !19
}

; Function Attrs: nounwind readnone speculatable willreturn
declare void @llvm.dbg.declare(metadata, metadata, metadata) #1

declare dso_local void @print(i32) #2

; Function Attrs: noinline nounwind optnone
define dso_local i32 @add_1(i32 %0) #0 !dbg !20 {
  %2 = alloca i32, align 4
  store i32 %0, i32* %2, align 4
  call void @llvm.dbg.declare(metadata i32* %2, metadata !21, metadata !DIExpression()), !dbg !22
  %3 = load i32, i32* %2, align 4, !dbg !23
  %4 = add nsw i32 %3, 1, !dbg !24
  ret i32 %4, !dbg !25
}

; Function Attrs: noinline nounwind optnone
define dso_local i32 @main() #0 !dbg !26 {
  call void @print_result(i32 (i32)* @add_1), !dbg !29
  ret i32 0, !dbg !30
}

attributes #0 = { noinline nounwind optnone "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "min-legal-vector-width"="0" "no-infs-fp-math"="false" "no-jump-tables"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }
attributes #1 = { nounwind readnone speculatable willreturn }
attributes #2 = { "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "no-infs-fp-math"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!3, !4, !5, !6}
!llvm.ident = !{!7}

!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, producer: "clang version 10.0.0-4ubuntu1 ", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, enums: !2, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "mcfunction.c", directory: "/home/salix/Documents/Minecraft/langcraft")
!2 = !{}
!3 = !{i32 1, !"NumRegisterParameters", i32 0}
!4 = !{i32 7, !"Dwarf Version", i32 4}
!5 = !{i32 2, !"Debug Info Version", i32 3}
!6 = !{i32 1, !"wchar_size", i32 4}
!7 = !{!"clang version 10.0.0-4ubuntu1 "}
!8 = distinct !DISubprogram(name: "print_result", scope: !1, file: !1, line: 42, type: !9, scopeLine: 42, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!9 = !DISubroutineType(types: !10)
!10 = !{null, !11}
!11 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !12, size: 32)
!12 = !DISubroutineType(types: !13)
!13 = !{!14, !14}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!15 = !DILocalVariable(name: "func", arg: 1, scope: !8, file: !1, line: 42, type: !11)
!16 = !DILocation(line: 42, column: 23, scope: !8)
!17 = !DILocation(line: 43, column: 11, scope: !8)
!18 = !DILocation(line: 43, column: 5, scope: !8)
!19 = !DILocation(line: 44, column: 1, scope: !8)
!20 = distinct !DISubprogram(name: "add_1", scope: !1, file: !1, line: 46, type: !12, scopeLine: 46, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!21 = !DILocalVariable(name: "num", arg: 1, scope: !20, file: !1, line: 46, type: !14)
!22 = !DILocation(line: 46, column: 15, scope: !20)
!23 = !DILocation(line: 47, column: 12, scope: !20)
!24 = !DILocation(line: 47, column: 16, scope: !20)
!25 = !DILocation(line: 47, column: 5, scope: !20)
!26 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 50, type: !27, scopeLine: 50, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!27 = !DISubroutineType(types: !28)
!28 = !{!14}
!29 = !DILocation(line: 51, column: 5, scope: !26)
!30 = !DILocation(line: 52, column: 1, scope: !26)
