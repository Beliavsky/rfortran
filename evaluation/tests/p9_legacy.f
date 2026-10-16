      PROGRAM P9
      IMPLICIT DOUBLE PRECISION (A-H,O-Z)
      DIMENSION A(10)
      COMMON /BLK/ N
      N = 10
      DO 10 I = 1, N
         A(I) = DBLE(I)
   10 CONTINUE
      CALL TOT(A, S)
      WRITE(*,100) S
  100 FORMAT(1X,'SUM=',F8.2)
      END
      SUBROUTINE TOT(A, S)
      IMPLICIT DOUBLE PRECISION (A-H,O-Z)
      DIMENSION A(*)
      COMMON /BLK/ N
      S = 0
      DO 20 I = 1, N
   20 S = S + A(I)
      END
