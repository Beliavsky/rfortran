# Warning examples

Each file here is a small, complete Fortran program demonstrating one of
rfortran's warnings, named after the warning: `unused-variable.f90`
demonstrates `-Wunused-variable`. The programs show what each warning
reports, and, as a set, they can serve as a benchmark for other compilers and
linters.

Each program starts with a header:

```fortran
! Warning:    unused-variable
! Enabled by: -Wall
! A local variable that is declared but never referenced: ...
```

`Enabled by` names the settings that report it: `default` (reported without
options), `-Wall`, `-Wextra`, `-Wstyle`, or `-W<category> only`. A line
`! gfortran: no (reason)` marks a program using a feature gfortran does not
support yet.

Every line that should be reported carries a comment naming the category:

```fortran
integer :: counter    ! expect: unused-variable
```

The other lines are deliberately similar code that should **not** be
reported, such as the same check guarded correctly, or a variable that is used.
A tool that reports them gives a false alarm.

## Running them

```cmd
rfort -Wunused-variable check unused-variable.f90
rfort -Wall check unused-variable.f90
```

Without installing rfortran, use the **try it** links below, which open the
program in the browser playground with its warning group selected, or
[`rfort_web_run.py`](../web-interpreter/rfort_web_run.py):

```cmd
python rfort_web_run.py -Wall --check unused-variable.f90
```

`tests/test_warning_examples.py` checks that rfortran reports each category
on exactly the marked lines, with its own option and with the group in the
header, that narrower settings report nothing, and that gfortran accepts the
programs.

## Using the set as a benchmark

To score another tool, run it on each file and compare the line numbers it
reports with the `! expect:` lines: a report on a marked line is a hit, a
missed marked line is a miss, and a report on any other line is a false
alarm. Matching by line needs no knowledge of the tool's messages or rule
names. Some programs contain code that other tools rightly report for other
reasons, so count only reports that concern the same problem.

The programs are valid Fortran unless the header says otherwise; most run,
but a few, such as `infinite-loop.f90`, demonstrate code that misbehaves when
run.

## Warnings

| Warning | Enabled by | What it reports | |
| --- | --- | --- | --- |
| [`action-mismatch`](action-mismatch.f90) | on by default | READ from a unit opened with ACTION="write", or WRITE to one opened with ACTION="read". | [try it](https://beliavsky.github.io/rfortran/?source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Faction-mismatch.f90) |
| [`allocation-state`](allocation-state.f90) | on by default | DEALLOCATE of an ALLOCATABLE that is not allocated, or ALLOCATE of one that already is, on every path to the statement. | [try it](https://beliavsky.github.io/rfortran/?source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fallocation-state.f90) |
| [`implicit-open`](implicit-open.f90) | on by default | READ or WRITE on a unit that no OPEN connects. | [try it](https://beliavsky.github.io/rfortran/?source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fimplicit-open.f90) |
| [`memory-leak`](memory-leak.f90) | on by default | Storage allocated through a local POINTER and never deallocated: it is lost when the procedure returns. | [try it](https://beliavsky.github.io/rfortran/?source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fmemory-leak.f90) |
| [`recursive-io`](recursive-io.f90) | on by default | An I/O statement in a function that is referenced by another I/O statement on the same unit. | [try it](https://beliavsky.github.io/rfortran/?source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Frecursive-io.f90) |
| [`aliasing`](aliasing.f90) | `-Wall` | The same variable, or overlapping parts of it, passed as two arguments when the procedure modifies one of them. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Faliasing.f90) |
| [`character-truncation`](character-truncation.f90) | `-Wall` | A CHARACTER value longer than the variable it is assigned to; the extra characters are silently dropped. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fcharacter-truncation.f90) |
| [`cmplx-kind`](cmplx-kind.f90) | `-Wall` | CMPLX without KIND= returns default COMPLEX even for double precision arguments, so cmplx(x, y) silently drops their extra digits. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fcmplx-kind.f90) |
| [`conformance`](conformance.f90) | `-Wall` | A call whose array arguments, of sizes known when compiling, break a size relationship that the procedure needs, as here x*y for x and y of different sizes. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fconformance.f90) |
| [`do-subscript`](do-subscript.f90) | `-Wall` | A subscript that a DO loop's range takes outside the array's declared bounds, as a(i) for i = 1 to 11 with a(10). | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fdo-subscript.f90) |
| [`empty-case`](empty-case.f90) | `-Wall` | A CASE range whose lower bound exceeds its upper bound, as case (5:1), matches nothing. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fempty-case.f90) |
| [`enum-exhaustive`](enum-exhaustive.f90) | `-Wall` | A SELECT CASE on an enumeration type, without CASE DEFAULT, that misses some of the type's values. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fenum-exhaustive.f90) |
| [`excess-real-digits`](excess-real-digits.f90) | `-Wall` | A REAL literal with more significant digits than its kind holds: a default REAL guarantees 6 decimal digits and REAL(real64) 15, so the default REAL 3.141592653589793 keeps only about 7 of its 16. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fexcess-real-digits.f90) |
| [`format`](format.f90) | `-Wall` | A format whose edit descriptors do not match the types of the items it formats, which stops the program with an error at run time. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fformat.f90) |
| [`implicit-save`](implicit-save.f90) | `-Wall` | A local variable initialized in its declaration, as `integer :: calls = 0`, has the SAVE attribute: it is set once, not on every call, and keeps its value between calls. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fimplicit-save.f90) |
| [`implicit-typing`](implicit-typing.f90) | `-Wall`, `-Wstyle` | A variable typed by the implicit rules, without a declaration. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fimplicit-typing.f90) |
| [`infinite-loop`](infinite-loop.f90) | `-Wall` | A DO loop without loop control, or DO WHILE (.true.), that nothing in its body can end: no EXIT, RETURN, STOP or branch out of it. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Finfinite-loop.f90) |
| [`integer-division`](integer-division.f90) | `-Wall` | An integer division that discards a remainder where a REAL result was probably wanted: a constant division such as 1/3, or an integer quotient converted to REAL. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Finteger-division.f90) |
| [`loop-runs-once`](loop-runs-once.f90) | `-Wall` | A DO loop whose body ends in EXIT, RETURN or STOP on every path, so it can never start a second iteration; usually the EXIT belongs inside an IF. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Floop-runs-once.f90) |
| [`merge-guard`](merge-guard.f90) | `-Wall` | MERGE may evaluate both values whatever the mask is, so the mask cannot guard them: merge(a(i), 0, i > 0) may still evaluate a(0). | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fmerge-guard.f90) |
| [`optional-argument`](optional-argument.f90) | `-Wall` | An OPTIONAL dummy argument used where nothing proves it is PRESENT. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Foptional-argument.f90) |
| [`pointer-init`](pointer-init.f90) | `-Wall` | A POINTER component of a derived type without a default initialization. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fpointer-init.f90) |
| [`precision-loss`](precision-loss.f90) | `-Wall` | A default REAL literal or calculation stored in a double precision variable: x = sqrt(2.0) computes in single precision and then widens the result, so x has only about 7 correct digits. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fprecision-loss.f90) |
| [`real-do-variable`](real-do-variable.f90) | `-Wall` | A DO loop with a REAL variable, deleted from the language in Fortran 95. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Freal-do-variable.f90) |
| [`short-circuit`](short-circuit.f90) | `-Wall` | Fortran does not promise to evaluate .AND. and .OR. left to right or to stop early, so `allocated(x) .and. size(x) > 0` may evaluate size(x) for an unallocated x. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fshort-circuit.f90) |
| [`target-lifetime`](target-lifetime.f90) | `-Wall` | A pointer associated with a local variable that ceases to exist when the procedure returns. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Ftarget-lifetime.f90) |
| [`underflow`](underflow.f90) | `-Wall` | A nonzero constant expression whose value is too small for its REAL kind and rounds to zero. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funderflow.f90) |
| [`unit-leak`](unit-leak.f90) | `-Wall` | OPEN(NEWUNIT=u) of a local unit variable in a procedure that never closes it. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funit-leak.f90) |
| [`unreachable-code`](unreachable-code.f90) | `-Wall` | Statements that can never run: after STOP, RETURN, EXIT or CYCLE, or in a branch whose condition is a constant. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funreachable-code.f90) |
| [`unused-but-set-variable`](unused-but-set-variable.f90) | `-Wall` | A local variable that is assigned but whose value is never read. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-but-set-variable.f90) |
| [`unused-dummy-argument`](unused-dummy-argument.f90) | `-Wall` | A dummy argument that the procedure never references. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-dummy-argument.f90) |
| [`unused-function`](unused-function.f90) | `-Wall` | A function or subroutine that nothing calls: an internal procedure, or a PRIVATE module procedure. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-function.f90) |
| [`unused-label`](unused-label.f90) | `-Wall` | A statement label that no GO TO, DO, FORMAT reference or I/O branch uses. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-label.f90) |
| [`unused-parameter`](unused-parameter.f90) | `-Wall` | A named constant that nothing references. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-parameter.f90) |
| [`unused-type`](unused-type.f90) | `-Wall` | A derived type that nothing uses: no variable, component, constructor or other type refers to it. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-type.f90) |
| [`unused-variable`](unused-variable.f90) | `-Wall` | A local variable that is declared but never referenced: often left over from an earlier version of the code, or a sign that a name is misspelled where it should have been used. | [try it](https://beliavsky.github.io/rfortran/?flags=Wall&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-variable.f90) |
| [`compare-reals`](compare-reals.f90) | `-Wextra` | == or /= with a REAL or COMPLEX operand. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fcompare-reals.f90) |
| [`constant-condition`](constant-condition.f90) | `-Wextra` | A condition, or part of one, whose value is known when compiling, so that part of the test has no effect: a literal combined with another operand, a variable compared with itself, or comparisons that contradict each other. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fconstant-condition.f90) |
| [`could-be-intent-in`](could-be-intent-in.f90) | `-Wextra` | An INTENT(INOUT) argument that nothing in the procedure can modify. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fcould-be-intent-in.f90) |
| [`could-be-intent-out`](could-be-intent-out.f90) | `-Wextra` | An INTENT(INOUT) argument whose value on entry is never used: every path assigns it before reading it. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fcould-be-intent-out.f90) |
| [`dead-store`](dead-store.f90) | `-Wextra` | An assignment to a local variable that a later assignment overwrites before anything reads it, so the first value is never used. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fdead-store.f90) |
| [`dimension-override`](dimension-override.f90) | `-Wextra` | An entity whose own shape or length overrides the DIMENSION attribute or the CHARACTER length of its declaration, as y(3) in `real, dimension(2) :: x, y(3)`. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fdimension-override.f90) |
| [`fixed-unit`](fixed-unit.f90) | `-Wextra` | An OPEN of a constant unit number in a procedure or module. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Ffixed-unit.f90) |
| [`format-reversion`](format-reversion.f90) | `-Wextra` | An output list with more items than the format has data edit descriptors, so the format is reused from its last group and the output continues on new records. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fformat-reversion.f90) |
| [`format-unused`](format-unused.f90) | `-Wextra` | Data edit descriptors that no item of the list reaches, which often means an item is missing. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fformat-unused.f90) |
| [`intent-out-unset`](intent-out-unset.f90) | `-Wextra` | An INTENT(OUT) argument that some path through the procedure does not assign, so the caller receives an undefined value. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fintent-out-unset.f90) |
| [`intrinsic-shadow`](intrinsic-shadow.f90) | `-Wextra` | A procedure with the name of an intrinsic procedure. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fintrinsic-shadow.f90) |
| [`keyword-name`](keyword-name.f90) | `-Wextra` | A variable, dummy argument or procedure named like a Fortran statement keyword or attribute, such as `block`, `exit`, `if` or `result`. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fkeyword-name.f90) |
| [`logical-comparison`](logical-comparison.f90) | `-Wextra`, `-Wstyle` | A LOGICAL value compared with .true. or .false.. `flag .eqv. .true.` is just `flag`, and `flag .eqv. .false.` is `.not. flag`. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Flogical-comparison.f90) |
| [`maybe-nonconforming`](maybe-nonconforming.f90) | `-Wextra` | An elemental operation on two arrays whose sizes depend on the data and may differ, as two PACK results with different masks. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fmaybe-nonconforming.f90) |
| [`maybe-out-of-bounds`](maybe-out-of-bounds.f90) | `-Wextra` | A constant subscript of a PACK result, which may have fewer elements than the subscript needs: pack(x, mask) is empty when no element of the mask is true. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fmaybe-out-of-bounds.f90) |
| [`maybe-uninitialized`](maybe-uninitialized.f90) | `-Wextra` | A local variable read where some path from the start of the procedure reaches the read without assigning it, so its value may be undefined. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fmaybe-uninitialized.f90) |
| [`missing-intent`](missing-intent.f90) | `-Wextra`, `-Wstyle` | A dummy argument without INTENT. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fmissing-intent.f90) |
| [`preconnected-unit`](preconnected-unit.f90) | `-Wextra` | READ from unit 5, or WRITE to unit 6 or 0. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fpreconnected-unit.f90) |
| [`redundant-cycle`](redundant-cycle.f90) | `-Wextra` | CYCLE as the last statement of a loop body, directly or at the end of a branch that ends the body, where the next iteration starts anyway. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fredundant-cycle.f90) |
| [`sequence-association`](sequence-association.f90) | `-Wextra` | A whole array passed to an explicit-shape dummy of another shape. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fsequence-association.f90) |
| [`shadow`](shadow.f90) | `-Wextra` | A local declaration that hides a variable or named constant of the host or of a used module. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fshadow.f90) |
| [`shadow-dummy-argument`](shadow-dummy-argument.f90) | `-Wextra` | A dummy argument with the name of a host variable or named constant, which it hides in the procedure. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Fshadow-dummy-argument.f90) |
| [`unchecked-conformance`](unchecked-conformance.f90) | `-Wextra` | A procedure that combines two assumed-shape arrays in a way that needs their sizes to match, as x*y or dot_product(x, y), without checking them. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funchecked-conformance.f90) |
| [`unchecked-status`](unchecked-status.f90) | `-Wextra` | An IOSTAT or STAT variable that nothing inspects afterwards. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funchecked-status.f90) |
| [`unused-only`](unused-only.f90) | `-Wextra` | A name in a USE statement's ONLY list that the scope never uses. | [try it](https://beliavsky.github.io/rfortran/?flags=Wextra&source=https%3A%2F%2Fraw.githubusercontent.com%2FBeliavsky%2Frfortran%2Fmain%2Fwarnings%2Funused-only.f90) |

## License

The programs in this directory are available under the MIT license (see
[LICENSE-MIT](../LICENSE-MIT)), so other projects may copy and adapt them.
