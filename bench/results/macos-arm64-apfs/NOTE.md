# Note on these results

Measured on a GitHub Actions `macos-latest` runner (a virtual machine), on the
runner's own APFS disk, for the small and medium fixtures only: the large
fixture's run did not complete within the job limit, and a script bug
(bash 3.2 and an empty array, since fixed) ended the job after medium.

Timings on this runner are **not reproducible** (17 of 113 values within
±10%); disk numbers mostly are. Treat the timings as indicative. The Linux
results in `../linux-x86_64-xfs/` are the reference (159 of 160 within ±10%).
