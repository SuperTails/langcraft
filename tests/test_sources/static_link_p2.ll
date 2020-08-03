; ModuleID = 'mcfunction.c'
source_filename = "mcfunction.c"
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i386-pc-linux-gnu"

@global_foo = external dso_local global i32, align 4

; Function Attrs: noinline nounwind optnone
define dso_local void @modify_foo() #0 !dbg !8 {
  %1 = load i32, i32* @global_foo, align 4, !dbg !11
  %2 = add nsw i32 %1, 22, !dbg !11
  store i32 %2, i32* @global_foo, align 4, !dbg !11
  ret void, !dbg !12
}

attributes #0 = { noinline nounwind optnone "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "min-legal-vector-width"="0" "no-infs-fp-math"="false" "no-jump-tables"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }

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
!8 = distinct !DISubprogram(name: "modify_foo", scope: !1, file: !1, line: 44, type: !9, scopeLine: 44, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!9 = !DISubroutineType(types: !10)
!10 = !{null}
!11 = !DILocation(line: 45, column: 16, scope: !8)
!12 = !DILocation(line: 46, column: 1, scope: !8)
