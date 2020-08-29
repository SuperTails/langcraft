; ModuleID = 'mcfunction.c'
source_filename = "mcfunction.c"
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i386-pc-linux-gnu"

%struct.Bar = type { i32 }
%struct.Foo = type { i32 }

@print_bar = dso_local local_unnamed_addr global void (%struct.Bar*)* bitcast (void (%struct.Foo*)* @print_foo to void (%struct.Bar*)*), align 4, !dbg !0

; Function Attrs: nounwind
define dso_local void @print_foo(%struct.Foo* nocapture readonly %0) #0 !dbg !24 {
  call void @llvm.dbg.value(metadata %struct.Foo* %0, metadata !32, metadata !DIExpression()), !dbg !33
  %2 = getelementptr inbounds %struct.Foo, %struct.Foo* %0, i32 0, i32 0, !dbg !34
  %3 = load i32, i32* %2, align 4, !dbg !34, !tbaa !35
  tail call void @print(i32 %3) #4, !dbg !40
  ret void, !dbg !41
}

; Function Attrs: nounwind readnone speculatable willreturn
declare void @llvm.dbg.declare(metadata, metadata, metadata) #1

declare !dbg !6 dso_local void @print(i32) local_unnamed_addr #2

; Function Attrs: nounwind
define dso_local i32 @main() local_unnamed_addr #0 !dbg !42 {
  %1 = alloca %struct.Foo, align 4
  %2 = bitcast %struct.Foo* %1 to i8*, !dbg !47
  call void @llvm.lifetime.start.p0i8(i64 4, i8* nonnull %2) #4, !dbg !47
  call void @llvm.dbg.declare(metadata %struct.Foo* %1, metadata !46, metadata !DIExpression()), !dbg !48
  %3 = getelementptr inbounds %struct.Foo, %struct.Foo* %1, i32 0, i32 0, !dbg !49
  store i32 42, i32* %3, align 4, !dbg !50, !tbaa !35
  call void @llvm.dbg.value(metadata %struct.Foo* %1, metadata !32, metadata !DIExpression()) #4, !dbg !51
  tail call void @print(i32 42) #4, !dbg !53
  %4 = load void (%struct.Bar*)*, void (%struct.Bar*)** @print_bar, align 4, !dbg !54, !tbaa !55
  %5 = bitcast %struct.Foo* %1 to %struct.Bar*, !dbg !57
  call void %4(%struct.Bar* nonnull %5) #4, !dbg !54
  call void @llvm.lifetime.end.p0i8(i64 4, i8* nonnull %2) #4, !dbg !58
  ret i32 0, !dbg !58
}

; Function Attrs: argmemonly nounwind willreturn
declare void @llvm.lifetime.start.p0i8(i64 immarg, i8* nocapture) #3

; Function Attrs: argmemonly nounwind willreturn
declare void @llvm.lifetime.end.p0i8(i64 immarg, i8* nocapture) #3

; Function Attrs: nounwind readnone speculatable willreturn
declare void @llvm.dbg.value(metadata, metadata, metadata) #1

attributes #0 = { nounwind "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="none" "less-precise-fpmad"="false" "min-legal-vector-width"="0" "no-infs-fp-math"="false" "no-jump-tables"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }
attributes #1 = { nounwind readnone speculatable willreturn }
attributes #2 = { "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="none" "less-precise-fpmad"="false" "no-infs-fp-math"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cx8,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }
attributes #3 = { argmemonly nounwind willreturn }
attributes #4 = { nounwind }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!19, !20, !21, !22}
!llvm.ident = !{!23}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "print_bar", scope: !2, file: !3, line: 54, type: !16, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C99, file: !3, producer: "clang version 10.0.0-4ubuntu1 ", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug, enums: !4, retainedTypes: !5, globals: !15, splitDebugInlining: false, nameTableKind: None)
!3 = !DIFile(filename: "mcfunction.c", directory: "/home/salix/Documents/Minecraft/langcraft")
!4 = !{}
!5 = !{!6, !11}
!6 = !DISubprogram(name: "print", scope: !7, file: !7, line: 1, type: !8, flags: DIFlagPrototyped, spFlags: DISPFlagOptimized, retainedNodes: !4)
!7 = !DIFile(filename: "./mcinterface.h", directory: "/home/salix/Documents/Minecraft/langcraft")
!8 = !DISubroutineType(types: !9)
!9 = !{null, !10}
!10 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!11 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !12, size: 32)
!12 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Bar", file: !3, line: 46, size: 32, elements: !13)
!13 = !{!14}
!14 = !DIDerivedType(tag: DW_TAG_member, name: "x", scope: !12, file: !3, line: 47, baseType: !10, size: 32)
!15 = !{!0}
!16 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !17, size: 32)
!17 = !DISubroutineType(types: !18)
!18 = !{null, !11}
!19 = !{i32 1, !"NumRegisterParameters", i32 0}
!20 = !{i32 7, !"Dwarf Version", i32 4}
!21 = !{i32 2, !"Debug Info Version", i32 3}
!22 = !{i32 1, !"wchar_size", i32 4}
!23 = !{!"clang version 10.0.0-4ubuntu1 "}
!24 = distinct !DISubprogram(name: "print_foo", scope: !3, file: !3, line: 50, type: !25, scopeLine: 50, flags: DIFlagPrototyped | DIFlagAllCallsDescribed, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !2, retainedNodes: !31)
!25 = !DISubroutineType(types: !26)
!26 = !{null, !27}
!27 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !28, size: 32)
!28 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Foo", file: !3, line: 42, size: 32, elements: !29)
!29 = !{!30}
!30 = !DIDerivedType(tag: DW_TAG_member, name: "x", scope: !28, file: !3, line: 43, baseType: !10, size: 32)
!31 = !{!32}
!32 = !DILocalVariable(name: "foo", arg: 1, scope: !24, file: !3, line: 50, type: !27)
!33 = !DILocation(line: 0, scope: !24)
!34 = !DILocation(line: 51, column: 16, scope: !24)
!35 = !{!36, !37, i64 0}
!36 = !{!"Foo", !37, i64 0}
!37 = !{!"int", !38, i64 0}
!38 = !{!"omnipotent char", !39, i64 0}
!39 = !{!"Simple C/C++ TBAA"}
!40 = !DILocation(line: 51, column: 5, scope: !24)
!41 = !DILocation(line: 52, column: 1, scope: !24)
!42 = distinct !DISubprogram(name: "main", scope: !3, file: !3, line: 56, type: !43, scopeLine: 56, flags: DIFlagAllCallsDescribed, spFlags: DISPFlagDefinition | DISPFlagOptimized, unit: !2, retainedNodes: !45)
!43 = !DISubroutineType(types: !44)
!44 = !{!10}
!45 = !{!46}
!46 = !DILocalVariable(name: "foo", scope: !42, file: !3, line: 57, type: !28)
!47 = !DILocation(line: 57, column: 5, scope: !42)
!48 = !DILocation(line: 57, column: 16, scope: !42)
!49 = !DILocation(line: 58, column: 9, scope: !42)
!50 = !DILocation(line: 58, column: 11, scope: !42)
!51 = !DILocation(line: 0, scope: !24, inlinedAt: !52)
!52 = distinct !DILocation(line: 59, column: 5, scope: !42)
!53 = !DILocation(line: 51, column: 5, scope: !24, inlinedAt: !52)
!54 = !DILocation(line: 60, column: 5, scope: !42)
!55 = !{!56, !56, i64 0}
!56 = !{!"any pointer", !38, i64 0}
!57 = !DILocation(line: 60, column: 16, scope: !42)
!58 = !DILocation(line: 61, column: 1, scope: !42)
