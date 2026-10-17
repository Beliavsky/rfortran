export const examples = [
  { name: 'Hello, Fortran', files: [{ name: 'main.f90', content: `program hello
   implicit none
   print *, "Hello from rfortran!"
end program hello
` }], stdin: '' },
  { name: 'Arrays & arithmetic', files: [{ name: 'main.f90', content: `program arrays
   implicit none
   real :: x(5)
   integer :: i
   x = [(real(i), i=1,5)]
   print *, "Values:", x
   print *, "Sum:", sum(x)
   print *, "Mean:", sum(x) / size(x)
   print *, "Dot product:", dot_product(x, x)
end program arrays
` }], stdin: '' },
  { name: 'Read some input', files: [{ name: 'main.f90', content: `program greeting
   implicit none
   integer :: n
   print *, "Enter an integer:"
   read *, n
   print *, "Its square is", n*n
end program greeting
` }], stdin: '12\n' },
  { name: 'Multiple source files', files: [{ name: 'math_mod.f90', content: `module math_mod
   implicit none
contains
   integer function twice(n)
      integer, intent(in) :: n
      twice = 2*n
   end function twice
end module math_mod
` }, { name: 'main.f90', content: `program modules
   use math_mod, only: twice
   implicit none
   print *, "Twice 21 is", twice(21)
end program modules
` }], stdin: '' }
];
