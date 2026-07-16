# Expected records

Exact diagnostic and generation records live here alongside their invalid,
package, and seeded-generation fixtures. `.diag` records contain one code and
source slice; `.diags` records preserve the ordered `code|source slice` sequence
from a recovering parse or package compilation. `.outputs` records pin generated
text and deterministic work counters for a sampler compatibility version.
