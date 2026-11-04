# Limitation repro index

This page maps every partial or missing row in [feature-status.md](feature-status.md)
to existing test methods, or identifies a gap. It reuses the tests rather than
creating a second collection of copied programs.

**Source audit: 2026-11-02.** There are 74 partial and 7 missing rows in this
snapshot. The feature marks below are copied from that page, not newly certified
support claims. Test source and assertions were inspected; the tests were **not
rerun** against the compiler being edited elsewhere. A test may be outdated too.

## Reading the index

- **Repro: rejection/runtime**: an existing method asserts the stated limitation.
- **Partial repro**: some subcases of the documented restriction have repros;
  the boundary column identifies what still lacks one.
- **Gap**: no specific limitation repro was identified in the cited tests and
  focused cross-file searches. Links on these rows are **related coverage only**,
  not proof that the unsupported form is tested. This is not a claim that no
  example exists anywhere else in the repository or external suites.
- **Review**: the status wording is too broad, conflicts with success cases, or
  cites a validation rule rather than a demonstrated missing feature. Check the
  documentation and tests before treating this as implementation work.

Some methods combine several programs. The boundary column identifies the
relevant subcase; invalid-program diagnostics in the same method are not
additional missing-feature repros. Processor-dependent kind availability is
also distinct from a required language feature.

The tables preserve the original feature-status sections. The original page's
Tests column remains the starting point for wider coverage; links here select
specific methods useful for the limitation audit. Source-line fragments locate
the current methods; retain the method names when line numbers move.

## Source form and declarations

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [Preprocessor conditionals (`#if`, `#ifdef`, `#else`, `#endif`)](language-support.md#preprocessor-conditionals) (partial) | `#include` and macro replacement in Fortran lines; the same method also tests malformed directives, which are not missing features. | [`test_preprocessor.py::Preprocessor.test_unsupported_forms_are_errors`](../tests/test_preprocessor.py#L107) | Repro: rejection |
| [BOZ literal constants](language-support.md#other-statement-forms) (partial) | No boundary repro for the documented patterns above 32 bits; the linked case exercises supported BOZ conversions and comparisons. | [`test_syntax_forms.py::SyntaxForms.test_boz_literals_and_unsigned_comparisons`](../tests/test_syntax_forms.py#L205) | Gap |
| [Declaration initialization](language-support.md#declaration-initialization) (partial) | Declaration initialization inside BLOCK has a rejection subcase. The saved CHARACTER array limit above 1024 elements still needs a dedicated boundary case. | [`test_main_initialization.py::MainInitializationTests.test_diagnostics_and_persistent_storage_boundary`](../tests/test_main_initialization.py#L79)<br>[`test_saved_locals.py::SavedLocals.test_saved_and_initialized_arrays`](../tests/test_saved_locals.py#L187) | Partial repro: rejection |
| SAVE (partial) | The status note says scalars only, but saved-array success cases exist. A separate rejection subcase covers saved TARGET storage; do not count invalid PURE/result initialization cases as feature gaps. | [`test_saved_locals.py::SavedLocals.test_saved_and_initialized_arrays`](../tests/test_saved_locals.py#L187)<br>[`test_saved_locals.py::SavedLocals.test_invalid_or_unsupported_saved_storage`](../tests/test_saved_locals.py#L233) | Review |
| [Statement labels](language-support.md#statement-labels) (partial) | The claim that only READ END= can branch is inconsistent with GOTO coverage. The labelled OPEN ERR= case demonstrates a separate I/O-control rejection. | [`test_goto_and_real_output.py::GotoAndRealOutput.test_goto`](../tests/test_goto_and_real_output.py#L22)<br>[`test_statement_labels.py::StatementLabels.test_err_branch_is_not_silently_ignored`](../tests/test_statement_labels.py#L81) | Review |
| COMMON blocks (partial) | The mixed CHARACTER/numeric COMMON case is rejected, but its language constraints need review before counting it as a valid unsupported form. Derived objects and SAVE /name/ need focused cases; the IMPLICIT NONE subcase is validation only. | [`test_common_blocks.py::CommonBlocks.test_rejections`](../tests/test_common_blocks.py#L97) | Review |
| BLOCK DATA (partial) | The cited rejection tests duplicate initialization of the same COMMON object, not the documented mixed storage-initializer type boundary. Existing storage tests are related success coverage. | [`test_block_data.py::BlockData.test_storage_and_equivalence`](../tests/test_block_data.py#L78)<br>[`test_block_data.py::BlockData.test_rejections`](../tests/test_block_data.py#L189) | Gap |
| [DATA statements](language-support.md#other-statement-forms) (partial) | The DATA note restricts changed procedure-local arrays; existing SAVE/DATA procedure tests should be reconciled with that wording before choosing a new failing case. No boundary repro identified. | [`test_external_calls.py::ExternalCalls.test_save_data_and_dummy_sections`](../tests/test_external_calls.py#L356)<br>[`test_statement_forms_io.py::StatementFormsIo.test_data_statements`](../tests/test_statement_forms_io.py#L75) | Review |
| [EQUIVALENCE](language-support.md#other-statement-forms) (partial) | Reading/passing mixed-type numeric equivalenced storage has a rejection case. The mixed CHARACTER/numeric subcase also needs a language-constraint review. Multidimensional sections and whole-array mixed-storage use are not individually mapped. | [`test_equivalence_storage.py::EquivalenceStorage.test_rejections`](../tests/test_equivalence_storage.py#L192) | Partial repro: rejection |
| [Named enum types (Fortran 2023)](fortran2023.md#enum-types-section-93) (partial) | No enum-specific C-call or runtime READ boundary repro identified. Integer enum I/O and fixed input lists are success coverage; type-conformance errors are validation tests. | [`test_enum_io.py::EnumIO.test_runtime_formats_sections_and_implied_do`](../tests/test_enum_io.py#L45)<br>[`test_formatted_read_lists.py::FormattedReadLists.test_enum_sections_and_implied_do`](../tests/test_formatted_read_lists.py#L90) | Gap |

## Types and kinds

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| INTEGER kind 16 (partial) | INTEGER(16) with HUGE is explicitly rejected. The kind-16 literal and pointer boundaries still need cases. Numeric kind values are processor conventions. | [`test_integer_huge.py::IntegerHuge.test_diagnostics`](../tests/test_integer_huge.py#L31) | Partial repro: rejection |
| REAL kind 16 (quad precision) (missing) | The REAL128 subcase checks the unavailable-kind diagnostic (`REAL128=-1`); it is a capability check, not a claim that quad precision is mandatory. No quad arithmetic case identified. | [`test_intrinsic_env.py::IntrinsicEnvironmentTests.test_diagnostics`](../tests/test_intrinsic_env.py#L97) | Partial repro: rejection |
| [COMPLEX kinds 4 and 8](language-support.md#complex) (partial) | No dedicated default-initialized COMPLEX component or input-section rejection found in the cited tests. Component success coverage is available. | [`test_complex.py::ComplexTests.test_components_of_derived_types`](../tests/test_complex.py#L29) | Gap |
| [CHARACTER](language-support.md#character-declarations-and-lengths) (partial) | The broad CHARACTER row delegates its runtime restrictions to more specific rows below. Its linked tests cover lengths and aliases, not one definitive remaining-limit repro. | [`test_character_lengths.py::CharacterLengths.test_lengths`](../tests/test_character_lengths.py#L28)<br>[`test_character_lengths.py::CharacterLengths.test_assignment_and_aliasing`](../tests/test_character_lengths.py#L32) | Gap |
| [ISO_FORTRAN_ENV](language-support.md#intrinsic-modules) (partial) | Missing ATOMIC_*, LOCK_TYPE, EVENT_TYPE and STAT_* entities have no specific repro identified. Existing ISO_FORTRAN_ENV entity tests exercise available entities. | [`test_intrinsic_rewrites.py::IntrinsicRewrites.test_iso_fortran_env_entities`](../tests/test_intrinsic_rewrites.py#L73) | Gap |
| [ISO_C_BINDING and C procedure bindings](language-support.md#c-interoperability) (partial) | BIND(C) CHARACTER/COMPLEX and other unsupported dummy forms have rejection subcases. Allocation-changing descriptors, assumed rank, C function pointers, LOGICAL and derived layouts are not all mapped by those cases. | [`test_c_interop.py::CInterop.test_rejected_features_and_standard`](../tests/test_c_interop.py#L526) | Partial repro: rejection |
| [IEEE_ARITHMETIC](language-support.md#intrinsic-modules) (partial) | IEEE rounding/exception control and other IEEE modules need specific cases. Scalar classification tests are related success coverage only. | [`test_ieee_arithmetic.py::IeeeArithmetic.test_scalar_classes_both_kinds`](../tests/test_ieee_arithmetic.py#L27) | Gap |

## Control flow

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [DO CONCURRENT](language-support.md#do-loops) (partial) | Allocatable and explicit runtime-shaped REDUCE storage have repros. Other locality specifiers and a loop index type-spec still need their own cases. Sequential execution is an implementation choice. | [`test_concurrent_reduce.py::ConcurrentReduce.test_unsupported_runtime_storage`](../tests/test_concurrent_reduce.py#L209)<br>[`test_concurrent_reduce.py::ConcurrentReduce.test_explicit_runtime_shape`](../tests/test_concurrent_reduce.py#L261) | Partial repro: rejection |
| [SELECT CASE](language-support.md#select-case-ranges) (partial) | The CHARACTER range `case('a':'z')` subcase exercises the limitation; overlapping ranges and nonconstant integer bounds in the same method are validation cases. | [`test_case_ranges.py::CaseRanges.test_diagnostics`](../tests/test_case_ranges.py#L31) | Repro: rejection |
| [ASSOCIATE](language-support.md#associate) (partial) | `e=>a(i)` followed by changing `i` inside ASSOCIATE. Updating the selected variable itself is supported, so the status note needs narrower wording. | [`test_associate.py::Associate.test_selector_modified_in_block_is_rejected`](../tests/test_associate.py#L45)<br>[`test_associate.py::Associate.test_selectors`](../tests/test_associate.py#L15) | Repro: rejection |
| [SELECT TYPE](language-support.md#select-type) (partial) | A non-ALLOCATABLE array SELECT TYPE selector (a CLASS array dummy) is explicitly rejected. Polymorphic ALLOCATABLE array selectors and CLASS(*) allocation are supported. | [`test_select_type.py::SelectType.test_array_selector_explicit_diagnostic`](../tests/test_select_type.py#L366)<br>[`test_polymorphic_allocatables.py::PolymorphicAllocatables.test_class_arrays_and_unlimited_objects`](../tests/test_polymorphic_allocatables.py#L187) | Partial repro: rejection |
| [SELECT RANK](language-support.md#assumed-rank-arguments-and-select-rank) (partial) | Non-ALLOCATABLE polymorphic assumed-rank selectors have a rejection case; ALLOCATABLE ones are supported. Duplicate RANK(*) guards are invalid syntax/semantics, not a repro for valid assumed-size RANK(*) dispatch. | [`test_assumed_rank_derived.py::DerivedAssumedRank.test_reject_polymorphic`](../tests/test_assumed_rank_derived.py#L353)<br>[`test_assumed_rank.py::AssumedRank.test_invalid_guards`](../tests/test_assumed_rank.py#L440) | Partial repro: rejection |
| [GOTO](language-support.md#goto) (partial) | The existing rejection concerns jumping into a construct; it should not be counted as proof of missing valid GOTO support. Existing ordinary GOTO cases succeed. | [`test_goto_and_real_output.py::GotoAndRealOutput.test_goto_into_a_construct_is_diagnosed`](../tests/test_goto_and_real_output.py#L85)<br>[`test_goto_and_real_output.py::GotoAndRealOutput.test_goto`](../tests/test_goto_and_real_output.py#L22) | Review |
| Assigned GOTO, arithmetic IF (missing) | No focused assigned-GOTO or arithmetic-IF case identified in the cited coverage; the row lists no test. | No focused case identified. | Gap |

## Arrays

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [Assumed-rank dummies `x(..)`](language-support.md#assumed-rank-arguments-and-select-rank) (partial) | Non-ALLOCATABLE polymorphic assumed-rank dummies are explicitly rejected; ALLOCATABLE ones are supported. C descriptor ABI and generic-interface assumed-rank limitations are not individually mapped. | [`test_assumed_rank_derived.py::DerivedAssumedRank.test_reject_polymorphic`](../tests/test_assumed_rank_derived.py#L353)<br>[`test_polymorphic_allocatables.py::PolymorphicAllocatables.test_nonallocatable_assumed_rank_is_rejected`](../tests/test_polymorphic_allocatables.py#L202) | Partial repro: rejection |
| [Assumed-size dummies `x(*)`](language-support.md#external-procedures-and-separate-compilation) (partial) | External assumed-size numeric/LOGICAL calls have success coverage. Missing type/context combinations need specific cases. | [`test_external_calls.py::ExternalCalls.test_separately_compiled_library`](../tests/test_external_calls.py#L134) | Gap |
| [Array element actuals for array dummies (sequence association)](language-support.md#external-procedures-and-separate-compilation) (partial) | External numeric/LOGICAL sequence association has success coverage. Unsupported type/context combinations are not isolated in a rejection repro. | [`test_external_calls.py::ExternalCalls.test_one_compilation`](../tests/test_external_calls.py#L146) | Gap |
| [TRANSFER](language-support.md#external-procedures-and-separate-compilation) (partial) | Scalar bit transfers and array transfers used in CALL are covered. No specific valid unsupported TRANSFER form is isolated. | [`test_equivalence_storage.py::EquivalenceStorage.test_scalar_transfer`](../tests/test_equivalence_storage.py#L40)<br>[`test_external_calls.py::ExternalCalls.test_transfer_and_automatic_arrays`](../tests/test_external_calls.py#L504) | Gap |
| [WHERE, ELSEWHERE, nested WHERE](language-support.md#where-and-logical-arrays) (partial) | A WHERE section destination (`a(:)=2`) is explicitly rejected. A CHARACTER destination needs its own case; mask/type/shape errors in the same method are validation only. | [`test_where.py::WhereTests.test_diagnostics`](../tests/test_where.py#L31) | Partial repro: rejection |

## ALLOCATABLE and POINTER

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [ALLOCATABLE dummy arguments](language-support.md#allocatable-dummy-arguments) (partial) | OPTIONAL polymorphic ALLOCATABLE descriptors have a rejection case. CHARACTER output calls have related working cases, but not every remaining function-dummy restriction has a repro. | [`test_polymorphic_allocatable_dummies.py::PolymorphicAllocatableDummies.test_optional_descriptor_diagnosed`](../tests/test_polymorphic_allocatable_dummies.py#L231)<br>[`test_character_allocatable_dummies.py::CharacterAllocatableDummies.test_local_output_with_bounded_input`](../tests/test_character_allocatable_dummies.py#L11) | Partial repro: rejection |
| [ALLOCATABLE function results](language-support.md#allocatable-function-results) (partial) | Direct/mutual scalar recursion and runtime CHARACTER lengths have success coverage. A specific remaining result-storage/argument boundary needs to be isolated. | [`test_scalar_result_extensions.py::ScalarResultExtensions.test_recursive_runtime_character`](../tests/test_scalar_result_extensions.py#L122)<br>[`test_numeric_allocatable_results.py::NumericAllocatableResults.test_direct_assignment`](../tests/test_numeric_allocatable_results.py#L11) | Gap |
| [ALLOCATABLE scalars](language-support.md#allocatable-scalars) (partial) | Recursive scalar-derived ALLOCATABLE dummy descriptors have a rejection case in finalization tests. Explicit SAVE on intrinsic allocatables, DO control variables and other listed limits are not all mapped. | [`test_allocatable_scalar_finalization.py::AllocatableScalarFinalization.test_recursive_descriptor_limit_diagnosed`](../tests/test_allocatable_scalar_finalization.py#L312) | Partial repro: rejection |
| [Reallocation on assignment](language-support.md#reallocation-on-assignment) (partial) | Rank-two numeric reallocation is covered. No focused repro found for rank-two CHARACTER reallocation or mixed fixed/runtime multidimensional expressions. The method named unsupported consumers mostly contains success cases and an invalid-rank rejection. | [`test_allocatable_reallocation.py::AllocatableReallocation.test_rank_two_allocation_from_arrays_and_sections`](../tests/test_allocatable_reallocation.py#L75)<br>[`test_runtime_allocatables.py::RuntimeAllocatables.test_unsupported_consumers_rejected`](../tests/test_runtime_allocatables.py#L219) | Gap |
| [MOVE_ALLOC](language-support.md#move_alloc-and-allocate-options) (partial) | Derived MOVE_ALLOC has success coverage, but no derived-array STAT/ERRMSG boundary repro was identified. Intrinsic-array copying is an implementation detail unless a semantic/performance failure is demonstrated. | [`test_derived_move_alloc.py::DerivedMoveAlloc.test_fixture`](../tests/test_derived_move_alloc.py#L28) | Gap |
| [CHARACTER and derived-type pointers](language-support.md#derived-type-and-character-pointers) (partial) | Deferred-length CHARACTER pointers and pointer association with a substring are explicitly rejected. Assumed-length pointer dummies and POINTER results need individual cases. | [`test_pointer_allocation.py::PointerAllocation.test_unsupported_character_pointers`](../tests/test_pointer_allocation.py#L351) | Partial repro: rejection |

## CHARACTER

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [Deferred-length `character(len=:), allocatable` scalars](language-support.md#deferred-length-scalar-character) (partial) | No focused deferred-length CHARACTER dummy rejection was identified. Long heap-backed local strings have success cases; unrelated substring validation is not evidence of this limit. | [`test_runtime_character_storage.py::RuntimeCharacterStorage.test_grow_shrink_and_self_assignment`](../tests/test_runtime_character_storage.py#L50)<br>[`test_deferred_character_locals.py::DeferredCharacterLocals.test_procedure_locals`](../tests/test_deferred_character_locals.py#L20) | Gap |
| [Deferred-length CHARACTER arrays](language-support.md#deferred-length-scalar-character) (partial) | Array dummy/result/module storage and the documented 4096-character array limit need focused cases. Runtime array lengths are covered by positive tests. | [`test_deferred_character_arrays.py::DeferredCharacterArrays.test_runtime_length_allocation`](../tests/test_deferred_character_arrays.py#L16) | Gap |
| Automatic CHARACTER lengths `character(len=n)` (missing) | Marked missing, but its own note and linked tests show LEN(dummy)-based automatic lengths working. General `character(len=n)` needs a distinct remaining-limit repro. | [`test_dummy_character_lengths.py::DummyCharacterLengths.test_runtime_contents_local_arrays_and_length_arithmetic`](../tests/test_dummy_character_lengths.py#L14) | Review |
| [CHARACTER functions](language-support.md#scalar-character-functions) (partial) | The documented one-character-at-a-time result-write failure has no isolated case in the cited file. That file instead rejects recursive CHARACTER functions; distinguish the two issues and recheck the old runtime claim. | [`test_character_functions.py::CharacterFunctions.test_diagnostics`](../tests/test_character_functions.py#L144)<br>[`test_character_functions.py::CharacterFunctions.test_fixed_lengths_named_nested_calls_and_returns`](../tests/test_character_functions.py#L18) | Review |

## Derived types

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [Derived array constructors with scalar and array-valued items](language-support.md#arrays-of-derived-type) (partial) | Owned-component and implied-DO derived constructors need dedicated cases; current constructor tests exercise runtime/fixed inputs and components that work. | [`test_derived_constructors.py::DerivedConstructors.test_fixed_array_components`](../tests/test_derived_constructors.py#L132)<br>[`test_derived_constructors.py::DerivedConstructors.test_runtime_multidimensional_source`](../tests/test_derived_constructors.py#L112) | Gap |
| Recursive derived types (linked lists, trees) (partial) | Extending a recursive derived type has a rejection case. Polymorphism, separate module initialization and storage-pool lifetime limits need separate evidence. | [`test_recursive_types.py::RecursiveTypes.test_rejections`](../tests/test_recursive_types.py#L275) | Partial repro: rejection |
| [Scalar polymorphic ALLOCATABLE components](language-support.md#scalar-polymorphic-allocatable-components) (partial) | Recursive polymorphic component layouts and polymorphic ALLOCATABLE component arrays have explicit cases. Unlimited-polymorphic allocation layouts are not separately mapped. | [`test_polymorphic_components.py::PolymorphicComponents.test_recursive_layout_limit_is_explicit`](../tests/test_polymorphic_components.py#L264)<br>[`test_polymorphic_components.py::PolymorphicComponents.test_array_component_limit_is_explicit`](../tests/test_polymorphic_components.py#L274) | Partial repro: rejection |
| [Scalar polymorphic POINTER components](language-support.md#scalar-polymorphic-pointer-components) (partial) | Array/unlimited-polymorphic POINTER components and polymorphic SOURCE/MOLD allocation models have explicit rejections. The long status row lists additional receiver, recursion, result and lifetime boundaries; these cases do not cover all of them. | [`test_polymorphic_pointer_components.py::PolymorphicPointerComponents.test_interfaces_and_unsupported_descriptors_are_explicit`](../tests/test_polymorphic_pointer_components.py#L124)<br>[`test_polymorphic_pointer_allocation_models.py::AllocationModels.test_polymorphic_model_has_specific_diagnostic`](../tests/test_polymorphic_pointer_allocation_models.py#L228) | Partial repro: rejection |
| Nonpolymorphic ALLOCATABLE scalar and deferred-length CHARACTER components (partial) | No separate rejection case identified for the remaining polymorphic/recursive scalar-owned component layouts. Intrinsic and derived scalar component storage has related success coverage. | [`test_allocatable_scalar_components.py::AllocatableScalarComponents.test_derived_scalar_component`](../tests/test_allocatable_scalar_components.py#L60) | Gap |
| [Arrays of derived type](language-support.md#arrays-of-derived-type), including elements as procedure arguments and in type-bound calls (partial) | Derived pointer arrays with CHARACTER or ALLOCATABLE components have explicit rejection cases. Other pointer-component layouts still need individual cases. | [`test_derived_array_pointers.py::DerivedArrayPointers.test_unsupported_component_layout_is_diagnosed`](../tests/test_derived_array_pointers.py#L183) | Partial repro: rejection |
| [Scalar dynamic binding dispatch](language-support.md#scalar-dynamic-binding-dispatch) (partial) | No focused repro for non-ALLOCATABLE polymorphic array receivers. Scalar dispatch, polymorphic pointer dispatch and element calls of polymorphic ALLOCATABLE arrays (`test_polymorphic_allocatables.py`) are related success coverage. | [`test_dynamic_dispatch.py::DynamicDispatch.test_functions_subroutines_inherited_binding_and_self_dispatch`](../tests/test_dynamic_dispatch.py#L13) | Gap |
| [FINAL procedures](language-support.md#scalar-finalization) (partial) | Finalizable function results, polymorphic objects and defined assignment have explicit rejection cases. Inheritance, pointer finalization and recursive ownership are not comprehensively mapped. | [`test_finalization.py::Finalization.test_main_array_and_unsupported_result`](../tests/test_finalization.py#L271)<br>[`test_finalization.py::Finalization.test_defined_assignment_diagnosed`](../tests/test_finalization.py#L31) | Partial repro: rejection |
| [Type extension (EXTENDS)](language-support.md#type-extension) (partial) | The note lists supported extension forms without isolating the remaining limit. Inherited constructors work; define the unsupported form before adding a repro. | [`test_inherited_constructors.py::InheritedConstructors.test_fixture`](../tests/test_inherited_constructors.py#L42) | Gap |
| [TYPEOF declarations](language-support.md#typeof-and-classof-declarations) (partial) | TYPEOF of a finalizable object has an explicit rejection case in the finalization tests. Other storage limits remain unspecified; successful component cases do not demonstrate those limits. | [`test_finalization.py::Finalization.test_main_array_and_unsupported_result`](../tests/test_finalization.py#L271) | Partial repro: rejection |
| [CLASSOF declarations](language-support.md#typeof-and-classof-declarations) (partial) | The `classof(a),allocatable::b` subcase with a CLASS(*) dummy is explicitly rejected. Other CLASSOF allocation/dispatch forms need separate cases. | [`test_referenced_types.py::ReferencedTypes.test_invalid_references_and_classof_constraints`](../tests/test_referenced_types.py#L294) | Partial repro: rejection |
| [CLASS(*)](language-support.md#select-type) (partial) | The linked array-actual rejection passes an array to a scalar CLASS(*) dummy: it is rank-mismatch validation, not a missing array-dummy repro. Non-ALLOCATABLE CLASS(*) array dummies still need focused cases; CLASS(*) ALLOCATABLE objects are covered by `test_polymorphic_allocatables.py`. | [`test_select_type_intrinsic.py::IntrinsicSelectType.test_array_actual_rejected`](../tests/test_select_type_intrinsic.py#L381) | Gap |
| [Scalar polymorphic POINTER objects](language-support.md#scalar-polymorphic-pointer-objects) (partial) | A polymorphic POINTER dummy has an explicit descriptor-limit rejection. Pointer results, allocation, arrays and persistent objects still need separate cases. | [`test_polymorphic_pointers.py::PolymorphicPointers.test_pointer_dummy_limit_is_explicit`](../tests/test_polymorphic_pointers.py#L282) | Partial repro: rejection |
| [Scalar polymorphic ALLOCATABLE objects](language-support.md#scalar-polymorphic-allocatable-objects) (partial) | Saved polymorphic ALLOCATABLE scalars have a rejection case (arrays are now supported; that subcase checks missing ALLOCATE bounds); OPTIONAL polymorphic descriptors are checked separately. These do not exhaust every persistent/module form, nor module or function-result polymorphic ALLOCATABLE arrays. | [`test_polymorphic_allocatable.py::PolymorphicAllocatable.test_unsupported_storage_diagnostics`](../tests/test_polymorphic_allocatable.py#L173)<br>[`test_polymorphic_allocatable_dummies.py::PolymorphicAllocatableDummies.test_optional_descriptor_diagnosed`](../tests/test_polymorphic_allocatable_dummies.py#L231) | Repro: rejection |
| [ABSTRACT types and DEFERRED bindings](language-support.md#abstract-types-and-deferred-bindings) (partial) | No focused valid nondefault PASS(arg) abstract-interface example was identified. Contract/type mismatch and missing-implementation tests check invalid programs, not missing support. | [`test_abstract_types.py::AbstractTypes.test_shapes_fixture`](../tests/test_abstract_types.py#L29) | Gap |
| Parameterized derived types (partial) | Whole-array component access with runtime LEN parameters has a rejection subcase. Runtime-LEN extension and rank-two arrays still need dedicated cases; missing KIND defaults in the same method are validation only. | [`test_parameterized_types.py::ParameterizedTypes.test_rejections`](../tests/test_parameterized_types.py#L280) | Partial repro: rejection |
| Procedure-pointer components (partial) | No focused component-as-procedure-actual or impure array-target rejection identified. Existing component callbacks exercise supported forms. | [`test_procedure_pointer_components.py::ProcedurePointerComponents.test_pass_callbacks_and_arrays`](../tests/test_procedure_pointer_components.py#L57) | Gap |

## Procedures

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [External procedures, compiled separately](language-support.md#external-procedures-and-separate-compilation) (partial) | Separate numeric/intrinsic external calls have success coverage. Derived/procedure actual limitations have no isolated valid failing case here. | [`test_external_calls.py::ExternalCalls.test_separately_compiled_library`](../tests/test_external_calls.py#L134) | Gap |
| [RECURSIVE](language-support.md#recursive-procedures) (partial) | Borrowed-array recursion and recursive scalar results have success coverage. No remaining ordinary value-result array-dummy restriction is isolated. | [`test_recursive_arrays.py::RecursiveArrays.test_original_reproducer`](../tests/test_recursive_arrays.py#L14)<br>[`test_scalar_result_extensions.py::ScalarResultExtensions.test_recursive_integer_result`](../tests/test_scalar_result_extensions.py#L84) | Gap |
| [Internal procedures with host association](language-support.md#internal-procedures-and-host-association) (partial) | No focused valid failing case identified for automatic/dummy/deferred-length or recursive-host whole-object association. Existing recursive-helper tests are success coverage; SIZE(scalar) and INTENT(IN) errors do not prove these limits. | [`test_internal_procedures.py::InternalProcedures.test_recursive_array_helpers`](../tests/test_internal_procedures.py#L14)<br>[`test_internal_procedures.py::InternalProcedures.test_unsupported_host_definition_paths_are_rejected`](../tests/test_internal_procedures.py#L314) | Gap |
| [Dummy procedures](language-support.md#dummy-procedures-and-procedure-pointers) (partial) | Intrinsic procedures passed as actual procedures need a focused repro. Existing abstract-interface forwarding is success coverage; passing a data object as a procedure is validation only. | [`test_procedure_arguments.py::ProcedureArguments.test_abstract_interface_procedure_dummy`](../tests/test_procedure_arguments.py#L72) | Gap |
| [Procedure pointers](language-support.md#dummy-procedures-and-procedure-pointers) (partial) | Passing a procedure pointer as an actual argument needs a focused repro. Local pointer association/calls are covered. | [`test_procedure_arguments.py::ProcedureArguments.test_procedure_pointers`](../tests/test_procedure_arguments.py#L164) | Gap |
| ENTRY statements (partial) | Alternate returns and listed ENTRY result-type boundaries need cases. ENTRY in the main program is invalid and is not a missing-feature repro. | [`test_entry_statements.py::EntryStatements.test_function_entries_and_results`](../tests/test_entry_statements.py#L51)<br>[`test_entry_statements.py::EntryStatements.test_rejections`](../tests/test_entry_statements.py#L150) | Gap |

## Modules

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| [Submodules](usage.md#submodules) (partial) | Compiling the ancestor alone after the combined-source success case is explicitly rejected. This is a partial separate-compilation repro, not a full ancestor-object/child-object build; repeated-name handling needs its own case. | [`test_submodules.py::Submodules.test_files_compiled_together`](../tests/test_submodules.py#L222) | Partial repro: rejection |

## Input and output

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| DC decimal comma; RU, RD, RZ, RN, RC, RP rounding modes (missing) | No focused decimal-comma or directed-rounding descriptor repro identified; the row lists no test. Split DC and each distinct rounding boundary when adding cases. | No focused case identified. | Gap |
| [Internal WRITE to a CHARACTER variable](language-support.md#formatted-output) (partial) | The valid multi-record internal WRITE subcase reports multiple records unsupported. The preceding type/descriptor mismatch is validation only; whole-array internal files need another case. | [`test_internal_write.py::InternalWrite.test_invalid_runtime_format_and_multiple_records`](../tests/test_internal_write.py#L113) | Partial repro: runtime |
| [Formatted READ](language-support.md#file-and-standard-input) (partial) | Runtime implied-DO bounds have a rejection case. Constant-format input is covered; the existing runtime-format error uses malformed `bad`, not a valid unsupported numeric descriptor. Runtime numeric formats and other dynamic destinations need cases. | [`test_formatted_read_lists.py::FormattedReadLists.test_runtime_bounds_diagnosed`](../tests/test_formatted_read_lists.py#L181)<br>[`test_runtime_read_format.py::RuntimeReadFormat.test_invalid_format_without_iostat`](../tests/test_runtime_read_format.py#L89) | Partial repro: rejection |
| [OPEN, CLOSE, REWIND, BACKSPACE for sequential formatted files](language-support.md#file-and-standard-input) (partial) | Reconnection, STATUS=new on an existing file, and recoverable non-OPEN statement errors need isolated cases. Initial-position and failed-OPEN success/error-handling tests are related coverage. | [`test_open_position.py::OpenPosition.test_append_newunit_and_initial_position_only`](../tests/test_open_position.py#L21)<br>[`test_open_position.py::OpenPosition.test_failed_open_leaves_newunit_and_reports_message`](../tests/test_open_position.py#L159) | Gap |
| [INQUIRE](language-support.md#file-and-standard-input) (partial) | IOLENGTH and other omitted INQUIRE specifiers need cases. Existing validation uses invalid controls/types, not valid unsupported properties. | [`test_inquire.py::Inquire.test_unit_states`](../tests/test_inquire.py#L14)<br>[`test_inquire.py::Inquire.test_validation`](../tests/test_inquire.py#L253) | Gap |
| [Unformatted stream access](language-support.md#file-and-standard-input) (partial) | COMPLEX stream I/O needs a dedicated case. Supported intrinsic/derived stream roundtrips are covered; unsupported control tests do not cover the missing COMPLEX element type. | [`test_stream_io.py::StreamIO.test_round_trip_of_size_and_array`](../tests/test_stream_io.py#L18) | Gap |
| Unformatted sequential and direct access (partial) | An OPEN with ACCESS=direct has a rejection subcase outside the row's empty Tests column. A complete direct-access REC= roundtrip still needs a case. | [`test_file_io.py::FileIO.test_compile_time_diagnostics`](../tests/test_file_io.py#L104) | Partial repro: rejection |
| [NAMELIST](language-support.md#namelist) (partial) | The ALLOCATABLE NAMELIST object subcase is explicitly rejected. Internal output, sections, pointer/derived members and USE-associated groups need separate cases; the other bad-control examples are validation only. | [`test_namelist.py::Namelist.test_bad_controls_and_unsupported_storage`](../tests/test_namelist.py#L148) | Partial repro: rejection |
| [Defined derived-type I/O](language-support.md#defined-derived-type-output) (partial; updated 2026-11-04) | [test_defined_io.py](../tests/test_defined_io.py): `test_reject_unsupported_parent_forms` rejects mixed DT formats; file units and parent/child IOSTAT/IOMSG have success/error tests; `test_positioning_limit_is_diagnosed` checks child T/TL positioning; `test_reject_vector_subscripts` checks the remaining vector-section boundary; `test_array_dt_format_still_rejected` covers explicitly formatted arrays. Whole-array and triplet-section list-directed output is supported, including runtime-sized arrays. [Standalone success case](../tests/programs/xdefined_formatted_output.f90). | Read/unformatted transfers, generic interface blocks, and other documented boundaries still need focused repros. New suite was run in interpreter/native O0/O2 modes. | Partial repro |

## Intrinsic procedures

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| `ABS`, `MOD`, `MODULO`, `MIN`, `MAX`, `INT`, `NINT`, `REAL`, `DBLE`, `FLOOR`, `CEILING` (partial) | No exact runtime-value INT(...,KIND=...) boundary repro identified. Rounding KIND validation is related coverage but not evidence of the stated INT limitation. | [`test_rounding.py::Rounding.test_validation`](../tests/test_rounding.py#L98)<br>[`test_numeric_minmax.py::NumericMinmax.test_numeric_types_and_kinds`](../tests/test_numeric_minmax.py#L17) | Gap |
| [Bessel functions](language-support.md#elemental-math-functions) `BESSEL_J0`, `BESSEL_J1`, `BESSEL_JN`, `BESSEL_Y0`, `BESSEL_Y1`, `BESSEL_YN` (partial) | Transformational BESSEL_JN/BESSEL_YN bounds and REAL(4) result-kind/precision behavior need separate cases. Existing scalar Bessel examples are success coverage. | [`test_more_intrinsics.py::MoreIntrinsics.test_bessel_norm2_out_of_range_and_specific_names`](../tests/test_more_intrinsics.py#L52) | Gap |
| [`OUT_OF_RANGE`](language-support.md#integer-bit-intrinsics) (partial) | Array OUT_OF_RANGE and nonconstant ROUND need cases. The mixed-intrinsics test exercises working scalar forms. | [`test_more_intrinsics.py::MoreIntrinsics.test_bessel_norm2_out_of_range_and_specific_names`](../tests/test_more_intrinsics.py#L52) | Gap |
| `MERGE` (partial) | Derived-array sources/array masks and the fixed-CHARACTER 1024 expansion boundary need cases. Runtime CHARACTER arrays and scalar-derived MERGE success cases are related coverage. | [`test_character_merge.py::CharacterMerge.test_runtime_large_arrays_and_nondefault_bounds`](../tests/test_character_merge.py#L64)<br>[`test_derived_merge.py::DerivedMerge.test_xacv_reproducer`](../tests/test_derived_merge.py#L11) | Gap |
| [`MATMUL`](language-support.md#allocatable-arrays-and-pointers) (partial) | The operand restriction is stale or too broad: runtime strided-section and overlap cases already succeed. LOGICAL MATMUL still needs its own boundary repro; nonconforming shapes are validation only. | [`test_runtime_matmul.py::RuntimeMatmul.test_strided_sections_and_overlap`](../tests/test_runtime_matmul.py#L108)<br>[`test_runtime_matmul.py::RuntimeMatmul.test_mixture_section_reproducer`](../tests/test_runtime_matmul.py#L103) | Review |
| [`STORAGE_SIZE`](language-support.md#integer-bit-intrinsics) (partial) | Types with ALLOCATABLE/POINTER components need a STORAGE_SIZE case. Existing fixed-layout success cases do not cover that restriction. | [`test_storage_size.py::StorageSize.test_intrinsic_and_derived_layouts`](../tests/test_storage_size.py#L13) | Gap |
| `TRANSFER`, `IS_CONTIGUOUS` (partial) | See the TRANSFER row above for its related tests. No Fortran IS_CONTIGUOUS repro identified; CFI_is_contiguous C descriptor tests exercise a different interface. | No focused case identified. | Gap |

## Parallel features

| Feature / listed status | Boundary or review item | Existing test method(s) | Evidence |
|---|---|---|---|
| Coarrays (missing) | No focused coarray declaration, image-control or multi-image repro identified. These need separate scope and an appropriate reference execution environment. | No focused case identified. | Gap |
| OpenMP and OpenACC directives (missing) | No focused ignored-directive repro identified. Compilation success alone cannot demonstrate OpenMP/OpenACC execution; choose an observable directive effect and reference flags. | No focused case identified. | Gap |

## Using an existing repro

Many tests embed their Fortran source directly in the named Python method;
others read a `.f90` fixture or generate several source files. Read the linked
method and its helper to identify the particular case, inputs and execution
modes. Do not copy only a fragment that depends on a module constant such as
`HEADER` or `PREFIX` in the Python file.

For example, in Windows CMD, select an already-built compiler explicitly and
run the ASSOCIATE limitation case:

```cmd
set "RFORT_TEST_COMPILER=C:\rust\public_domain\github\rfortran\target\release\rfort.exe"
python tests\test_associate.py Associate.test_selector_modified_in_block_is_rejected
```

The compiler path must exist. This command does not rebuild Rust. An existing
negative test **passes when rfort rejects its program**; it does not mean the
Fortran feature works. Tests differ in whether they use the checker, interpreter,
native -O0/-O2 or gfortran; inspect the helper rather than assuming every method
runs every mode. No test command was executed as part of this documentation audit.

## Generated Fortran copies

To browse the embedded sources as ordinary Fortran files, run:

```cmd
python tools\extract_test_sources.py
```

The [extractor](../tools/extract_test_sources.py) writes generated copies under
`target/extracted-fortran/`, grouped by Python test file and test method. For
example, the changing-index ASSOCIATE repro is:

```text
target/extracted-fortran/test_associate/Associate.test_selector_modified_in_block_is_rejected/case_001.f90
```

`manifest.json` maps each copy to its original method and source line, records
the helper call and resolved arguments (including input/expected-output strings
where available), and lists unresolved expressions or methods with no extracted
source. `README.md` in the generated directory explains how to use the files.
Fixed-form filenames retain a fixed-form extension; preprocessed sources retain
their upper-case extension. Generation does not change or import the test files,
execute test methods, or run/build a compiler.

The extractor statically resolves string literals, module constants (including
local test imports), simple string-building helpers and bounded Python loops.
It reads repository fixtures when a linked method uses one. Both branches of
Python conditionals may be represented. Temporary-directory variables become
the current working directory in generated code; run experiments in a fresh
scratch directory, because some programs create or delete data files.

These are source payload copies, not a second authoritative test suite. Some
are intentionally invalid programs. Others require companion modules, C code,
data files or compiler flags from the original test. Files in one method
directory are not necessarily a single source group. The captured arguments
are helper-specific, not universally interpreted as a standalone run command.
This extractor is not an exhaustive evaluator for arbitrary Python helpers;
consult the original test when context is missing.

Refresh after test/index changes with:

```cmd
python tools\extract_test_sources.py --overwrite
```

Refresh refuses to replace modified generated files or unrelated output files;
save experimental edits elsewhere or use `--output target/my-extraction` for
a fresh directory. The generated copies live under ignored `target/`; commit
the extractor and documentation, not the snapshots. To check the extractor
itself without running any Fortran tests:

```cmd
python tools\test_extract_test_sources.py
```

## Maintaining the map

When a limitation is implemented, change its rejection/runtime-failure test into
a success regression with an expected result, then update this page and
feature-status.md together. Do not preserve an unsupported-feature assertion
solely to keep a historical negative test passing.

For an uncovered restriction, first isolate a valid program and its expected
behavior. Add it to the existing feature test file when practical; a standalone
fixture is useful for multi-file or longer repros. Link that method here instead
of duplicating the source in a parallel suite. One short example need not cover
every distinct restriction in a large partial-feature row.

Review the SAVE, statement-label, automatic-CHARACTER-length and runtime-MATMUL
notes early: their existing success tests already make the old wording
questionable. The ASSOCIATE note should distinguish modifying the selected
variable from changing the index/bounds used to select it.
