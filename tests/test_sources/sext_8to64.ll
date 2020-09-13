; ModuleID = 'sext_8to64.c'
source_filename = "sext_8to64.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

@a = dso_local global i8 127, align 1
@b = dso_local global i8 -128, align 1

; Function Attrs: noinline nounwind optnone sspstrong uwtable
define dso_local i32 @main() #0 {
  %1 = load i8, i8* @a, align 1
  %2 = sext i8 %1 to i64
  %3 = trunc i64 %2 to i32
  call void @print(i32 %3)
  %4 = load i8, i8* @a, align 1
  %5 = sext i8 %4 to i64
  %6 = lshr i64 %5, 32
  %7 = trunc i64 %6 to i32
  call void @print(i32 %7)
  %8 = load i8, i8* @b, align 1
  %9 = sext i8 %8 to i64
  %10 = trunc i64 %9 to i32
  call void @print(i32 %10)
  %11 = load i8, i8* @b, align 1
  %12 = sext i8 %11 to i64
  %13 = lshr i64 %12, 32
  %14 = trunc i64 %13 to i32
  call void @print(i32 %14)
  ret i32 0
}

declare void @print(i32) #1

attributes #0 = { noinline nounwind optnone sspstrong uwtable "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "min-legal-vector-width"="0" "no-infs-fp-math"="false" "no-jump-tables"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }
attributes #1 = { "correctly-rounded-divide-sqrt-fp-math"="false" "disable-tail-calls"="false" "frame-pointer"="all" "less-precise-fpmad"="false" "no-infs-fp-math"="false" "no-nans-fp-math"="false" "no-signed-zeros-fp-math"="false" "no-trapping-math"="false" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "unsafe-fp-math"="false" "use-soft-float"="false" }

!llvm.module.flags = !{!0, !1, !2}
!llvm.ident = !{!3}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 7, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{!"clang version 10.0.1 "}
