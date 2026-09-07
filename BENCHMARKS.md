# Benchmarks

Every number here says which machine produced it, on what date, and how many
runs it is the median of. Treat them as evidence that the kernels do what
they claim, not as a specification: three machines, three classes of
processor, and a class none of them belongs to can behave differently.

Read [Reproducing](#reproducing) before comparing anything against your own
run. Several things about this comparison will otherwise mislead you, and one
of them cost a day here.

## What it is measured against

| | what it is |
|---|---|
| `base64-simd` | Nugine's crate, 135M downloads, SIMD on x86-64, aarch64 and wasm |
| `base64-turbo` | the newest entrant, AVX2, AVX-512 and NEON kernels |
| `base64` (simd engine) | the standard crate's `engine::simd::Simd`, which picks NEON or AVX2 at first use |
| `base64` | the same crate's `general_purpose::STANDARD`, which is what a caller who writes `BASE64_STANDARD` actually gets |
| `firedancer` | `fd_base64.c` from Firedancer, vendored and compiled by clang at `-O3 -march=native` |

Firedancer's base64 is scalar C with no vector path on any architecture, so
it is measured on every machine rather than only on x86: its encoder walks
the input a byte at a time through a bit accumulator, which is why it lands
where it does.

The `base64` crate is measured twice on purpose. Its SIMD engines have to be
constructed by hand, and the constant nearly every caller reaches for is still
the scalar one, and the gap between those two rows is the cost of that.

## Reproducing

The benchmarks are their own crate, so that the published one carries neither
them nor the crates they measure against.

```sh
cd bench
cargo bench --bench against     # this crate against the other five
cargo bench --bench quick       # six sizes against two rivals, for a change
cargo bench --bench tail        # the fixed cost, in ns, at eight short lengths
cargo bench --bench shape       # where the time goes at small sizes
cargo bench --bench huge        # 1 MiB to 128 MiB, across L2, L3 and memory
```

`tools/report.py` turns one or more runs into the tables below, taking the
median of each cell where there is more than one. `tools/median.py` does the
same for a `quick` run as a ratio against a rival measured in the same run,
`tools/shape.py`, `tools/tail.py` and `tools/huge.py` read their own benches,
and `tools/x86-bench.sh` raises an x86 machine to run any of it on.

Six things to know before reading a number:

1. **Nothing here allocates.** Every row converts into a buffer made before
   the timing starts, because that is the only way the six are comparable:
   some have allocating APIs and some do not. A benchmark that times
   `encode_to_string` is timing the allocator.
2. **The harness costs about 0.7 ns.** At 12 and 32 bytes that is a real part
   of what is reported. `cargo bench --bench shape` has a `nothing` row that
   measures it alone, so it can be subtracted.
3. **Throughput hides fixed cost.** Below about 64 bytes almost nothing is
   being measured except the cost of the call. `cargo bench --bench tail`
   reports that in nanoseconds per call, and it is the only view that makes a
   change to it visible. A 1.4 ns regression sat in this crate for hours
   because every A/B in use started at 12 bytes and read GiB/s.
4. **Adding code changes the numbers of code that did not change.** A block
   tier that a given length never reaches still moved that length by several
   per cent here, through inlining and layout.
5. **A ratio is only meaningful when both sides run the same instruction
   set.** On a machine with AVX-512, `base64-turbo` dispatches to it while a
   build of this crate without the `avx512` feature runs AVX2, and comparing
   those two produces a number that means nothing. The AVX2 table below is
   from a machine with no AVX-512 for exactly this reason.
6. **Some machines are far noisier than others.** The M4 repeats within about
   2% when it is idle, and one run taken while other work was still settling
   read a decode row 29% slow with the encode rows beside it close to true,
   so gate a run on a quiet machine before believing a single figure from it.
   The AVX-512 machines moved by 25% between runs of identical code, so
   every figure in that table is soft and the tables that matter are medians.

## Machines

| | |
|---|---|
| **M4** | Apple M4 Max, macOS 26.5, `aarch64-apple-darwin`, rustc 1.93.1, NEON |
| **Milan** | AMD EPYC Milan, 8 dedicated cores (Hetzner ccx33), 32 MiB L3, Debian 12, rustc 1.98, AVX2, no AVX-512 |
| **Turin** | AMD EPYC 9B45, 8 vCPU (GCP c4d-standard-8), Debian 12, rustc 1.98, AVX-512 with VBMI |

A fourth, an Intel Xeon Platinum 8481C (GCP c3, Sapphire Rapids, AVX-512 with
VBMI), was used for correctness only and carries no table here: its run
predates the AVX-512 decode rewrite and its decode figures are superseded.
It confirmed the wide path on a second vendor's implementation of AVX-512.

A fifth, an Intel Xeon Skylake-SP (Hetzner cx23), exists in this record for
one reason: it reports AVX-512F, BW and VL but *not* VBMI, which is the case
the cpuid guard exists for. It declines the wide path and runs AVX2, which is
what should happen.

All built with `lto = "fat"`, `codegen-units = 1`, one benchmark at a time.

## Apple M4 Max, NEON

<!-- RESULTS -->

Median of five runs, 2026-08-22.

### encode (GiB/s)

| bytes | tape-base64 | base64-simd | base64-turbo | base64-simd-engine | base64 | firedancer | vs best other |
|---|---|---|---|---|---|---|---|
| 32 | 6.13 | 4.57 | 7.20 | 3.36 | 3.51 | 1.18 | **0.85x** |
| 64 | 11.61 | 9.48 | 10.49 | 3.47 | 3.68 | 1.10 | **1.11x** |
| 128 | 17.30 | 12.04 | 11.44 | 10.76 | 4.22 | 1.16 | **1.44x** |
| 1232 | 21.71 | 10.99 | 10.93 | 11.63 | 4.55 | 1.20 | **1.87x** |
| 4096 | 21.81 | 10.83 | 10.87 | 11.41 | 4.56 | 1.20 | **1.91x** |
| 16384 | 21.96 | 10.86 | 10.86 | 11.33 | 4.58 | 1.21 | **1.94x** |
| 65536 | 22.02 | 10.87 | 10.87 | 11.37 | 4.38 | 1.21 | **1.94x** |
| 1048576 | 22.01 | 10.83 | 10.88 | 11.40 | 4.43 | 1.21 | **1.93x** |

### decode (GiB/s)

| bytes | tape-base64 | base64-simd | base64-turbo | base64-simd-engine | base64 | firedancer | vs best other |
|---|---|---|---|---|---|---|---|
| 32 | 6.33 | 7.02 | 6.13 | 2.17 | 2.18 | 3.29 | **0.90x** |
| 64 | 8.59 | 8.11 | 7.65 | 4.58 | 2.83 | 3.48 | **1.06x** |
| 128 | 11.15 | 8.29 | 7.66 | 6.22 | 3.43 | 3.61 | **1.34x** |
| 1232 | 12.39 | 7.77 | 7.35 | 7.10 | 4.03 | 3.68 | **1.60x** |
| 4096 | 12.62 | 7.93 | 7.41 | 7.15 | 4.09 | 3.74 | **1.59x** |
| 16384 | 12.73 | 7.92 | 7.42 | 7.13 | 4.13 | 3.76 | **1.61x** |
| 65536 | 12.75 | 7.94 | 7.43 | 7.10 | 3.85 | 3.70 | **1.61x** |
| 1048576 | 12.66 | 7.93 | 7.34 | 6.98 | 3.90 | 3.72 | **1.60x** |

### The tail, M4

`cargo bench --bench tail`, ns per call, 2026-09-07, two interleaved rounds
each way. The tail writer used to divide the kernel's return value back into
an input offset, which made it wait on that value; indexing both slices from
the caller's group count instead lets the writer start beside the kernel.

| bytes | encode before | encode after | decode before | decode after |
|---|---|---|---|---|
| 24 | 3.30 | 3.08 | 3.58 | 3.51 |
| 32 | 4.76 | 4.51 | 4.66 | 4.59 |
| 48 | 3.82 | 3.64 | 4.76 | 4.53 |
| 64 | 5.29 | 5.06 | 6.85 | 6.38 |

## AMD EPYC Milan, AVX2

Both this crate and `base64-turbo` run AVX2 here, because the machine has no
AVX-512. It is the only configuration in which the AVX2 comparison means
anything.

<!-- X86 RESULTS -->

Median of three runs, 2026-08-22.

### encode (GiB/s)

| bytes | tape-base64 | base64-simd | base64-turbo | base64-simd-engine | base64 | firedancer | vs best other |
|---|---|---|---|---|---|---|---|
| 32 | 4.48 | 3.13 | 3.24 | 1.85 | 1.99 | 1.33 | **1.38x** |
| 64 | 6.90 | 5.58 | 5.21 | 2.23 | 2.29 | 1.33 | **1.24x** |
| 128 | 12.37 | 8.33 | 11.14 | 8.63 | 2.69 | 1.39 | **1.11x** |
| 1232 | 22.96 | 11.44 | 22.62 | 10.62 | 3.00 | 1.39 | **1.01x** |
| 4096 | 23.12 | 11.17 | 22.94 | 10.27 | 3.03 | 1.39 | **1.01x** |
| 16384 | 23.57 | 11.43 | 23.61 | 10.53 | 3.05 | 1.39 | **1.00x** |
| 65536 | 23.52 | 11.46 | 23.51 | 10.56 | 3.03 | 1.39 | **1.00x** |
| 1048576 | 22.91 | 11.34 | 22.94 | 10.51 | 3.02 | 1.40 | **1.00x** |

### decode (GiB/s)

| bytes | tape-base64 | base64-simd | base64-turbo | base64-simd-engine | base64 | firedancer | vs best other |
|---|---|---|---|---|---|---|---|
| 32 | 3.31 | 4.27 | 3.13 | 1.15 | 1.25 | 1.56 | **0.77x** |
| 64 | 5.15 | 4.52 | 4.37 | 2.57 | 1.52 | 1.68 | **1.14x** |
| 128 | 9.63 | 8.41 | 9.42 | 6.08 | 2.01 | 1.74 | **1.02x** |
| 1232 | 18.93 | 8.54 | 17.42 | 13.62 | 2.45 | 1.75 | **1.09x** |
| 4096 | 19.21 | 8.42 | 17.43 | 14.14 | 2.51 | 1.78 | **1.10x** |
| 16384 | 20.02 | 8.52 | 17.96 | 15.20 | 2.51 | 1.79 | **1.11x** |
| 65536 | 20.22 | 8.57 | 18.03 | 15.49 | 2.52 | 1.78 | **1.12x** |
| 1048576 | 19.99 | 8.50 | 17.76 | 15.41 | 2.48 | 1.78 | **1.13x** |

## AMD EPYC Turin, AVX-512VBMI

Behind the `avx512` feature, which is on by default.

<!-- VBMI RESULTS -->

Median of three runs, 2026-08-22. This machine moves 25% between runs of
identical code, so every figure here is soft and only the medians are worth
reading.

### encode (GiB/s)

| bytes | tape-base64 | base64-simd | base64-turbo | base64-simd-engine | base64 | firedancer | vs best other |
|---|---|---|---|---|---|---|---|
| 32 | 5.79 | 3.52 | 4.65 | 2.20 | 2.36 | 1.59 | **1.24x** |
| 64 | 11.03 | 6.85 | 9.10 | 2.65 | 2.74 | 1.63 | **1.21x** |
| 128 | 20.77 | 9.91 | 17.25 | 10.29 | 3.20 | 1.66 | **1.20x** |
| 1232 | 77.85 | 13.55 | 77.13 | 12.62 | 3.59 | 1.67 | **1.01x** |
| 4096 | 88.69 | 13.04 | 93.12 | 12.01 | 3.59 | 1.68 | **0.95x** |
| 16384 | 112.68 | 13.44 | 118.16 | 12.46 | 3.62 | 1.69 | **0.95x** |
| 65536 | 76.27 | 13.58 | 75.67 | 12.52 | 3.61 | 1.68 | **1.01x** |
| 1048576 | 47.99 | 13.51 | 43.95 | 12.48 | 3.59 | 1.67 | **1.09x** |

### decode (GiB/s)

| bytes | tape-base64 | base64-simd | base64-turbo | base64-simd-engine | base64 | firedancer | vs best other |
|---|---|---|---|---|---|---|---|
| 32 | 4.71 | 5.21 | 4.15 | 1.40 | 1.49 | 1.93 | **0.90x** |
| 64 | 9.14 | 5.72 | 8.08 | 3.21 | 1.93 | 2.03 | **1.13x** |
| 128 | 16.83 | 9.93 | 14.87 | 7.23 | 2.41 | 2.08 | **1.13x** |
| 1232 | 69.53 | 10.20 | 71.71 | 16.92 | 2.97 | 2.10 | **0.97x** |
| 4096 | 76.72 | 9.99 | 82.45 | 17.62 | 2.99 | 2.13 | **0.93x** |
| 16384 | 88.21 | 10.11 | 98.57 | 18.98 | 3.03 | 2.13 | **0.89x** |
| 65536 | 74.48 | 10.12 | 74.21 | 19.27 | 3.00 | 2.12 | **1.00x** |
| 1048576 | 43.87 | 10.07 | 44.54 | 19.19 | 2.96 | 2.12 | **0.98x** |

## Sizes past the cache

The tables above stop at 1 MiB, where the whole working set still fits. This
one runs to 128 MiB on the Milan machine, whose last level cache is 32 MiB,
because that is where keeping the output in cache stops being the right answer.

An encode touches its input and an output a third larger again, so the pair
leaves a cache of size C at about three sevenths of C. `base64-turbo` switches
to non-temporal stores above a fixed 4 MiB, which its source says was measured
on a 9 MiB last level cache: three sevenths of 9 MiB is 3.9 MiB, so that
constant is right for the machine it was taken from and too low for anything
with more cache. Three sevenths of 32 MiB is 13.7 MiB, and the rows from 4 to
8 MiB below are the band where the threshold has fired but the data still fits.
This crate reads the cache size from cpuid and scales the threshold to it.

Criterion reuses one output buffer across iterations, so the destination is
warmer than a one-shot caller's would be. That is the condition least
favourable to writing past the cache, and it is the same harness every other
table here uses.

<!-- HUGE RESULTS -->

Milan, 2026-08-22. Last level cache 32 MiB, so this crate starts writing past
it at 13.7 MiB and `base64-turbo` at 4 MiB.

### encode (GiB/s)

| MiB | tape-base64 | base64-turbo | base64-simd | vs turbo |
|---|---|---|---|---|
| 1 | 23.11 | 23.09 | 11.39 | **1.00x** |
| 2 | 23.17 | 23.10 | 11.33 | **1.00x** |
| 4 | 23.01 | 21.43 | 11.34 | **1.07x** |
| 8 | 23.13 | 21.46 | 11.33 | **1.08x** |
| 16 | 21.49 | 21.46 | 11.13 | **1.00x** |
| 32 | 20.79 | 20.94 | 11.14 | **0.99x** |
| 64 | 20.67 | 20.67 | 11.41 | **1.00x** |
| 128 | 20.98 | 21.15 | 11.22 | **0.99x** |

### decode (GiB/s)

| MiB | tape-base64 | base64-turbo | base64-simd | vs turbo |
|---|---|---|---|---|
| 1 | 20.24 | 17.97 | 8.53 | **1.13x** |
| 2 | 20.18 | 17.93 | 8.50 | **1.13x** |
| 4 | 20.00 | 17.89 | 8.50 | **1.12x** |
| 8 | 19.99 | 17.88 | 8.48 | **1.12x** |
| 16 | 17.04 | 15.94 | 8.33 | **1.07x** |
| 32 | 16.53 | 15.57 | 8.40 | **1.06x** |
| 64 | 16.04 | 15.43 | 8.42 | **1.04x** |
| 128 | 14.71 | 14.88 | 8.45 | **0.99x** |

## What was tried and did not work

Kept here because a negative result costs the same to produce as a positive
one and is lost more easily.

| change | result |
|---|---|
| Cover a small remainder with the portable walk instead of an overlapping block | 4.72 -> 5.58 ns at 32 bytes. A group costs about three times as much in the walk as inside a block, so converting four groups twice beats converting two of them by hand. |
| Port the AVX-512 128-entry value table down to NEON, as two `vqtbl4q` | decode 4-13% slower at every size. `vqtbl4q` costs four uops because it reads four table registers. `vpermi2b` does the same lookup in one. The wide-table trick only pays where the lookup is cheap. |
| Unroll both AVX2 loops, eight at a time with grouped loads | flat to slightly worse. Reverted. |
| Gather the AVX2 decoder's error accumulator across a run | 15.2 -> 14.5 GiB/s. On x86 `vptest` is cheap and the branch predicts. The same change is worth 9% on aarch64, where reading the accumulator crosses out of the vector registers. |
| Rewrite the AVX2 encoder in `base64-turbo`'s shape (first block manufacturing its own left padding with `vpermd`, no per-block branch, eight-way unroll) | +7% encode at 16 KiB in a same-machine A/B, and did not close the gap. It could not: the gap was in the arithmetic under the loop, not in the loop. Kept for the simpler code. |
| Replace the AVX-512 decoder's masked 48-byte store with a 32 and a 16 | no change. |
| Inline the streaming run beside the ordinary one | 32 byte encode 4.44 -> 4.01 GiB/s with every rival flat in the same run. A second copy of a run that only executes above 13.7 MiB moved a length that never reaches it. |
| Push that second copy out of line with `#[inline(never)]` | worse again, 4.01 -> 3.59. A call where there had been none gives the function a frame and callee-saved registers to spill, and a short buffer pays that on the way in whether or not the call is taken. Asking the question one level up, and handing the kernel a `const` answer, is what keeps the ordinary form free of it. |
| Gate the streaming store on 32 byte alignment, so a whole register lands at once | never fired. The allocator hands back 16 byte alignment, header included, so the test was false on every buffer and the feature did nothing while looking present. Two 16 byte stores behind a 16 byte test work. |
| Read the NEON decoder's validity accumulator every eight blocks instead of four | flat. The decode rows moved 2.8% and the encode rows, which the constant cannot reach, moved 3.2% in the same A/B, so both are layout drift between builds rather than the change. Twenty-one cycles a block on the M4 sits at the sum of the resource bounds: twelve `tbl` at two a cycle, the rest of the arithmetic at four a cycle, one structured load and one structured store. What is left would need fewer lookups, and the wide-table route to that is the row above. |

## What was learned the hard way

A value where a constant would do cost real throughput in four separate
places: the AVX2 decode chunk length, the aarch64 decode run length, and both
AVX-512 mask computations. A run whose length is a value cannot unroll, and a
mask derived from a value has to be computed and moved into a mask register
every iteration. The AVX-512 encoder ran at 0.41x of a rival until its whole
blocks stopped deriving masks they already knew.

Reading the generated assembly found what no amount of reasoning did: LLVM
rewrites `_mm256_mulhi_epu16` by a power of two into a widen, two variable
shifts, a pack and a lane-crossing permute, seven instructions for one and
half the throughput.

The same rewrite, on the multiply that was actually shipped, cost a third of
the AVX2 encode throughput and hid for the length of the project. Both
multipliers are a power of two in each 16 bit lane, so LLVM turns each
`_mm256_mullo_epi16` into two shifts and a blend. Nine instructions where five
were written, and the four extra land on the two ports the two shuffles
already want: 8 uops a block on those ports rather than 3. Every attempt to
find it in the shape of the loop failed, because the loop was never the
problem. What found it was counting `vpmullw` in the output and getting zero.

The identical source is free on aarch64. `ushl` shifts each lane by its own
amount in one instruction, so the rewrite there is one for one. AVX2 has no
16 bit variable shift outside AVX-512BW. That difference is the whole of it,
and it is why the NEON encoder never showed the same loss.

`cargo test` cannot see a read past the end of a buffer, because an over-read
still produces the right answer. Two were found here, both by valgrind, and
both in code whose tests passed:

```sh
cargo test --release --test forced --no-run
valgrind --error-exitcode=9 --quiet target/release/deps/forced-*
```

Weakening the AVX2 encoder's bound by the four bytes it reads early leaves
every test passing and valgrind reporting `Invalid read of size 32`, which is
how that bound was settled.

A guard that is never true is indistinguishable from a feature that does not
help. The streaming store was written, compiled, reachable and valgrind clean,
and its alignment test was false on every buffer the benchmark allocated, so
the numbers came back unchanged and read as "non-temporal stores do not pay
here". What settled it was forcing the path on and off by hand in one process
and watching 16.66 GiB/s become 20.98. A switch worth adding is worth a way to
throw by hand.

Finally: check that an edit landed before believing a measurement of it.
Three changes reported here as "no improvement" had never been compiled. The
text they replaced had already been changed, the replacement silently matched
nothing, and three identical results were read as evidence rather than as the
alarm they were.
