10 REM Fibonacci sequence - first 10 numbers
20 LET A = 0
30 LET B = 1
40 LET N = 0
50 PRINT A
60 LET T = A + B
70 LET A = B
80 LET B = T
90 LET N = N + 1
100 IF N < 10 THEN 50
110 END
