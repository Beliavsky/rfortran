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
| `test_allocatable_scalar_components.py` `test_deferred_length_character`, `test_arrays_of_records`, `test_character_array_components_of_record_arrays` | Deferred-length CHARACTER components of records; a dozen statements take about 15 s |
| `test_deferred_character_components.py` `test_local_allocation_and_overlap` | Deferred-length CHARACTER array with a runtime length; about 12 s |
| `test_parameterized_types.py` `test_runtime_len_values` | Runtime LEN type parameters with CHARACTER components; about 12 s |
| `test_testsuite_fixes.py` `test_namelist_derived_members_module_groups_and_end` | Three NAMELIST reads and a write of groups with arrays of derived type and nested components; 7 to 11 s, while the compiled program takes 0.2 s |
| `test_namelist.py` `test_allocatable_pointer_and_assumed_shape_members` | A NAMELIST group of seven ALLOCATABLE, POINTER and derived-type members: each IF of the READ's dispatch copies the frame, so one READ of 25 values takes about 4 s (6 s for the program, over 15 s under the full suite's load), while the compiled program takes 0.13 s |

Output differences, not speed:

| Program or test | Difference |
|---|---|
| `print *, z` for a `complex(kind=8)` value | The interpreter prints the parts with single-precision digits, `(1.00000000,-1.00000000)`; compiled programs print double-precision digits as gfortran does |
