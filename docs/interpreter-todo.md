# Interpreter to-do

Development focuses on the compiler. Programs that are correct but too slow in
the interpreter (`rfort run`) are listed here, and their tests check only the
compiled -O0 and -O2 builds (with `interpreter=False` in
`test_deallocate.Deallocate.check`, or `build_commands(...)[1:]`).

Assignments and many generated statements copy the variables of the current
frame, so each statement costs time proportional to the frame's storage. Heap
CHARACTER arrays are stored as one INTEGER per character and each element access
goes through CHARACTER temporaries, so programs using them are the most affected.

| Program or test | Why it is slow |
|---|---|
| `test_character_arrays.py` `test_large_arrays` | CHARACTER arrays of more than 1024 elements, about 1 ms per element operation |
| `test_deferred_character_arrays.py` `test_assignment_reallocates_to_length_and_size` | Deferred-length arrays with a runtime length use temporaries of 4096 characters per element access (about 20 s) |
| `test_deferred_character_arrays.py` `test_values_that_refer_to_the_array` | As above |

Output differences, not speed:

| Program or test | Difference |
|---|---|
| `print *, z` for a `complex(kind=8)` value | The interpreter prints the parts with single-precision digits, `(1.00000000,-1.00000000)`; compiled programs print double-precision digits as gfortran does |
