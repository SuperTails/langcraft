; ModuleID = './tests/fibonacci.bc'
source_filename = "mcfunction.c"
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i386-pc-linux-gnu"

; Function Attrs: noinline nounwind optnone
define dso_local i32 @fibonnaci(i32 %0) #0 !dbg !8 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, i32* %3, align 4
  call void @llvm.dbg.declare(metadata i32* %3, metadata !12, metadata !DIExpression()), !dbg !13
  %4 = load i32, i32* %3, align 4, !dbg !14
  %5 = icmp eq i32 %4, 0, !dbg !16
  br i1 %5, label %6, label %7, !dbg !17

6:                                                ; preds = %1
  store i32 0, i32* %2, align 4, !dbg !18
  br label %19, !dbg !18

7:                                                ; preds = %1
  %8 = load i32, i32* %3, align 4, !dbg !20
  %9 = icmp eq i32 %8, 1, !dbg !22
  br i1 %9, label %10, label %11, !dbg !23

10:                                               ; preds = %7
  store i32 1, i32* %2, align 4, !dbg !24
  br label %19, !dbg !24

11:                                               ; preds = %7
  %12 = load i32, i32* %3, align 4, !dbg !26
  %13 = sub nsw i32 %12, 1, !dbg !28
  %14 = call i32 @fibonnaci(i32 %13), !dbg !29
  %15 = load i32, i32* %3, align 4, !dbg !30
  %16 = sub nsw i32 %15, 2, !dbg !31
  %17 = call i32 @fibonnaci(i32 %16), !dbg !32
  %18 = add nsw i32 %14, %17, !dbg !33
  store i32 %18, i32* %2, align 4, !dbg !34
  br label %19, !dbg !34

19:                                               ; preds = %11, %10, %6
  %20 = load i32, i32* %2, align 4, !dbg !35
  ret i32 %20, !dbg !35
}

; Function Attrs: nounwind readnone speculatable willreturn
declare void @llvm.dbg.declare(metadata, metadata, metadata) #1

; Function Attrs: noinline nounwind optnone
define dso_local i32 @main() #0 !dbg !36 {
  %1 = alloca i32, align 4
  %2 = alloca i32, align 4
  store i32 0, i32* %1, align 4
  call void @llvm.dbg.declare(metadata i32* %2, metadata !39, metadata !DIExpression()), !dbg !41
  store i32 0, i32* %2, align 4, !dbg !41
  br label %3, !dbg !42

3:                                                ; preds = %9, %0
  %4 = load i32, i32* %2, align 4, !dbg !43
  %5 = icmp slt i32 %4, 10, !dbg !45
  br i1 %5, label %6, label %12, !dbg !46

6:                                                ; preds = %3
  %7 = load i32, i32* %2, align 4, !dbg !47
  %8 = call i32 @fibonnaci(i32 %7), !dbg !49
  call void @print(i32 %8), !dbg !50
  br label %9, !dbg !51

9:                                                ; preds = %6
  %10 = load i32, i32* %2, align 4, !dbg !52
  %11 = add nsw i32 %10, 1, !dbg !52
  store i32 %11, i32* %2, align 4, !dbg !52
  br label %3, !dbg !53, !llvm.loop !54

12:                                               ; preds = %3
  %13 = load i32, i32* %1, align 4, !dbg !56
  ret i32 %13, !dbg !56
}

declare dso_local void @print(i32) #2

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
!8 = distinct !DISubprogram(name: "fibonnaci", scope: !1, file: !1, line: 5, type: !9, scopeLine: 5, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!9 = !DISubroutineType(types: !10)
!10 = !{!11, !11}
!11 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!12 = !DILocalVariable(name: "n", arg: 1, scope: !8, file: !1, line: 5, type: !11)
!13 = !DILocation(line: 5, column: 19, scope: !8)
!14 = !DILocation(line: 6, column: 9, scope: !15)
!15 = distinct !DILexicalBlock(scope: !8, file: !1, line: 6, column: 9)
!16 = !DILocation(line: 6, column: 11, scope: !15)
!17 = !DILocation(line: 6, column: 9, scope: !8)
!18 = !DILocation(line: 7, column: 9, scope: !19)
!19 = distinct !DILexicalBlock(scope: !15, file: !1, line: 6, column: 17)
!20 = !DILocation(line: 8, column: 16, scope: !21)
!21 = distinct !DILexicalBlock(scope: !15, file: !1, line: 8, column: 16)
!22 = !DILocation(line: 8, column: 18, scope: !21)
!23 = !DILocation(line: 8, column: 16, scope: !15)
!24 = !DILocation(line: 9, column: 9, scope: !25)
!25 = distinct !DILexicalBlock(scope: !21, file: !1, line: 8, column: 24)
!26 = !DILocation(line: 11, column: 26, scope: !27)
!27 = distinct !DILexicalBlock(scope: !21, file: !1, line: 10, column: 12)
!28 = !DILocation(line: 11, column: 28, scope: !27)
!29 = !DILocation(line: 11, column: 16, scope: !27)
!30 = !DILocation(line: 11, column: 45, scope: !27)
!31 = !DILocation(line: 11, column: 47, scope: !27)
!32 = !DILocation(line: 11, column: 35, scope: !27)
!33 = !DILocation(line: 11, column: 33, scope: !27)
!34 = !DILocation(line: 11, column: 9, scope: !27)
!35 = !DILocation(line: 13, column: 1, scope: !8)
!36 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 52, type: !37, scopeLine: 52, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!37 = !DISubroutineType(types: !38)
!38 = !{!11}
!39 = !DILocalVariable(name: "i", scope: !40, file: !1, line: 53, type: !11)
!40 = distinct !DILexicalBlock(scope: !36, file: !1, line: 53, column: 5)
!41 = !DILocation(line: 53, column: 14, scope: !40)
!42 = !DILocation(line: 53, column: 10, scope: !40)
!43 = !DILocation(line: 53, column: 21, scope: !44)
!44 = distinct !DILexicalBlock(scope: !40, file: !1, line: 53, column: 5)
!45 = !DILocation(line: 53, column: 23, scope: !44)
!46 = !DILocation(line: 53, column: 5, scope: !40)
!47 = !DILocation(line: 54, column: 25, scope: !48)
!48 = distinct !DILexicalBlock(scope: !44, file: !1, line: 53, column: 34)
!49 = !DILocation(line: 54, column: 15, scope: !48)
!50 = !DILocation(line: 54, column: 9, scope: !48)
!51 = !DILocation(line: 55, column: 5, scope: !48)
!52 = !DILocation(line: 53, column: 29, scope: !44)
!53 = !DILocation(line: 53, column: 5, scope: !44)
!54 = distinct !{!54, !46, !55}
!55 = !DILocation(line: 55, column: 5, scope: !40)
!56 = !DILocation(line: 56, column: 1, scope: !36)
