# Native output investigation inputs

These are frozen copies of the deterministic cases that previously produced
intermittent zero-exit/empty-output results. Their `.txt` files contain expected
stdout for `tests/stress_native_output.py`.

| Source | Original check |
| --- | --- |
| `loop.f90` | `test_inherited_constructors.InheritedConstructors.test_loop_condition_parent_is_repeated` |
| `owned_component.f90` | `test_polymorphic_pointer_allocated_targets.AllocatedPointerTargets.test_owned_component` |
| `named_kind.f90` | `test_polymorphic_pointer_owner_bindings.PointerOwnerBindings.test_named_kind_overloads` |

`control.c` prints `42` using the host C library and returns zero.
`control_status.c` returns 17 if `printf` does not report writing three bytes.
Both use `control.txt` as expected stdout. Build these independently with GCC;
they use no rfortran code or runtime.

Commands, captured results and the limits of the current conclusions are in
[the testing guide](../../../docs/testing.md#retaining-and-investigating-native-failures).
