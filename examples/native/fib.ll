declare i32 @printf(i8*, ...)
@.int_fmt = private unnamed_addr constant [4 x i8] c"%d\0A\00"
@.true_str = private unnamed_addr constant [6 x i8] c"true\0A\00"
@.false_str = private unnamed_addr constant [7 x i8] c"false\0A\00"

define i32 @fib(i32 %arg_n) {
entry:
  %n = alloca i32
  store i32 %arg_n, i32* %n
  %t1 = load i32, i32* %n
  %t2 = icmp slt i32 %t1, 2
  br i1 %t2, label %then1, label %else1
then1:
  %t3 = load i32, i32* %n
  ret i32 %t3
else1:
  br label %merge1
merge1:
  %t4 = load i32, i32* %n
  %t5 = sub i32 %t4, 1
  %t6 = call i32 @fib(i32 %t5)
  %t7 = load i32, i32* %n
  %t8 = sub i32 %t7, 2
  %t9 = call i32 @fib(i32 %t8)
  %t10 = add i32 %t6, %t9
  ret i32 %t10
}

define void @kairo_main() {
entry:
  %t1 = call i32 @fib(i32 10)
  %t2 = getelementptr [4 x i8], [4 x i8]* @.int_fmt, i32 0, i32 0
  %t3 = call i32 (i8*, ...) @printf(i8* %t2, i32 %t1)
  ret void
}

define i32 @main() {
entry:
  call void @kairo_main()
  ret i32 0
}