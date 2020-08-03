; ModuleID = 'mcfunction.c'
source_filename = "mcfunction.c"
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i386-pc-linux-gnu"

@global_foo = dso_local global i32 20, align 4, !dbg !0

; Function Attrs: noinline nounwind optnone
define dso_local i32 @main() #0 !dbg !12 {
  call void @modify_foo(), !dbg !15
  %1 = load i32, i32* @global_foo, align 4, !dbg !16
  call void @print(i32 %1), !dbg !17
  ret i32 0, !dbg !18
}

declare dso_local void @modify_foo() #1

declare dso_local void @print(i32) #1

attributes #0 = { noinline nounwind optnone "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "min-legal-vector-width"="0" "no-infs-fp-math"="false" "no-jump-tables"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }
attributes #1 = { "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "no-infs-fp-math"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!7, !8, !9, !10}
!llvm.ident = !{!11}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "global_foo", scope: !2, file: !3, line: 42, type: !6, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C99, file: !3, producer: "clang version 10.0.0-4ubuntu1 ", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, enums: !4, globals: !5, splitDebugInlining: false, nameTableKind: None)
!3 = !DIFile(filename: "mcfunction.c", directory: "/home/salix/Documents/Minecraft/langcraft")
!4 = !{}
!5 = !{!0}
!6 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!7 = !{i32 1, !"NumRegisterParameters", i32 0}
!8 = !{i32 7, !"Dwarf Version", i32 4}
!9 = !{i32 2, !"Debug Info Version", i32 3}
!10 = !{i32 1, !"wchar_size", i32 4}
!11 = !{!"clang version 10.0.0-4ubuntu1 "}
!12 = distinct !DISubprogram(name: "main", scope: !3, file: !3, line: 46, type: !13, scopeLine: 46, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !4)
!13 = !DISubroutineType(types: !14)
!14 = !{!6}
!15 = !DILocation(line: 47, column: 5, scope: !12)
!16 = !DILocation(line: 48, column: 11, scope: !12)
!17 = !DILocation(line: 48, column: 5, scope: !12)
!18 = !DILocation(line: 49, column: 1, scope: !12)
