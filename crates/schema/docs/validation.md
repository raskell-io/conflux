# Schema Validation

## Overview

Conflux validates config state at two levels:

1. **Field-level** — Type checking, range constraints, enum values, regex patterns
2. **Document-level** — Cross-entity invariants (referential integrity, uniqueness)

## Field Validation

Applied per-field based on schema attributes:

- **Type checking**: Value must match declared type
- **Range**: Numeric values within `range="min,max"`
- **Enum**: Value must be one of `values="a,b,c"`
- **Pattern**: String must match `pattern="regex"`
- **Required**: Field must have a non-null value

## Document Validation

Applied after merge to ensure the resolved state is consistent:

- **Referential integrity**: `ref` fields must point to existing entities
- **Uniqueness**: Entity IDs must be unique within their parent scope
- **Ordering**: Ordered children must have valid position indices

## Post-Merge Validation

After every merge, the resolved document is validated against the schema. If validation fails:

1. The merge result is flagged as **invalid**
2. The specific validation errors are attached
3. The milestone projector will not commit invalid state to git
4. The API returns the validation errors to the caller

This ensures that even though CRDTs guarantee convergence, they don't guarantee correctness — validation catches the gap.
