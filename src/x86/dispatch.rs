//! Which codec the running machine can execute, asked once and remembered

use core::arch::x86_64::CpuidResult;
#[cfg(not(miri))]
use core::arch::x86_64::{__cpuid, __cpuid_count, _xgetbv};
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Not yet asked
pub const UNKNOWN: u8 = 0;

/// Nothing beyond the architecture baseline
pub const PORTABLE: u8 = 1;

/// Vectors of 32 bytes
pub const AVX2: u8 = 2;

/// Byte permutes across a whole 64-byte register
pub const WIDE: u8 = 3;

static CHOSEN: AtomicU8 = AtomicU8::new(UNKNOWN);

/// The widest codec this machine can run
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Path {
    Portable,
    Avx2,
    Wide,
}

/// The path for this machine, probing the first time it is asked
#[inline]
pub(crate) fn path() -> Path {
    match CHOSEN.load(Ordering::Relaxed) {
        WIDE => Path::Wide,
        AVX2 => Path::Avx2,
        PORTABLE => Path::Portable,
        _ => settle(),
    }
}

#[cold]
fn settle() -> Path {
    let chosen = available();
    CHOSEN.store(chosen, Ordering::Relaxed);
    path_of(chosen)
}

fn path_of(chosen: u8) -> Path {
    match chosen {
        WIDE => Path::Wide,
        AVX2 => Path::Avx2,
        _ => Path::Portable,
    }
}

/// The widest path the build allows, where there is no `cpuid` to ask
#[cfg(miri)]
pub fn available() -> u8 {
    if cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512bw",
        target_feature = "avx512vbmi"
    )) {
        WIDE
    } else if cfg!(target_feature = "avx2") {
        AVX2
    } else {
        PORTABLE
    }
}

/// The widest path this machine can run, without recording it
#[cfg(not(miri))]
pub fn available() -> u8 {
    // SAFETY: each leaf and `xgetbv` is reached only once an earlier answer said it is there.
    unsafe {
        let highest = __cpuid(0).eax;
        if highest < 7 {
            return PORTABLE;
        }

        let leaf_one = __cpuid(1);
        let has_osxsave = leaf_one.ecx & (1 << 27) != 0;
        if !has_osxsave {
            return PORTABLE;
        }
        let state = _xgetbv(0);
        // The instructions are unusable unless the operating system saves the vector registers.
        let has_ymm_state = state & 0b110 == 0b110;
        if !has_ymm_state {
            return PORTABLE;
        }

        let leaf_seven = __cpuid_count(7, 0);
        if leaf_seven.ebx & (1 << 5) == 0 {
            return PORTABLE;
        }

        // Opmask, and both halves of the wide registers
        let has_zmm_state = state & 0b1110_0000 == 0b1110_0000;
        // Exactly the features the kernel is written in, since without VBMI `vpermb` faults.
        let has_vbmi = leaf_seven.ebx & (1 << 16) != 0     // avx512f
            && leaf_seven.ebx & (1 << 30) != 0             // avx512bw
            && leaf_seven.ecx & (1 << 1) != 0; // avx512vbmi
        if has_zmm_state && has_vbmi {
            WIDE
        } else {
            AVX2
        }
    }
}

/// Not yet asked
const UNMEASURED: usize = 0;

/// The range a last level cache descriptor has to land in to be believed
const SMALLEST_CACHE: usize = 1 << 16;
const LARGEST_CACHE: usize = 1 << 31;
const _: () = assert!(SMALLEST_CACHE / 7 * 3 > UNMEASURED);

static FLOOR: AtomicUsize = AtomicUsize::new(UNMEASURED);

/// The input length past which an encode should write past the cache, `usize::MAX` if unknown
pub(crate) fn stream_floor() -> usize {
    match FLOOR.load(Ordering::Relaxed) {
        UNMEASURED => {
            let floor = match last_level_bytes() {
                Some(bytes) => bytes / 7 * 3,
                None => usize::MAX,
            };
            FLOOR.store(floor, Ordering::Relaxed);
            floor
        }
        floor => floor,
    }
}

/// One `cpuid` leaf, or all zeros where the interpreter has no `cpuid`
// `cpuid` became a safe call after the msrv, where the block is still needed.
#[allow(unused_unsafe)]
fn cpuid(leaf: u32, index: u32) -> CpuidResult {
    #[cfg(miri)]
    {
        let _ = (leaf, index);
        CpuidResult {
            eax: 0,
            ebx: 0,
            ecx: 0,
            edx: 0,
        }
    }
    #[cfg(not(miri))]
    // SAFETY: every caller has read the leaf that says this one is there.
    unsafe {
        __cpuid_count(leaf, index)
    }
}

/// Bytes in the last level cache, or nothing if the processor will not say
fn last_level_bytes() -> Option<usize> {
    let extended = cpuid(0x8000_0000, 0).eax;
    // AMD describes its caches on 0x8000001D, Intel on 4, in one shape.
    let leaf = if extended >= 0x8000_001D {
        0x8000_001D
    } else if cpuid(0, 0).eax >= 4 {
        4
    } else {
        return None;
    };
    let mut largest = 0;
    for index in 0..16 {
        let cache = cpuid(leaf, index);
        // A type of zero means there are no more.
        if cache.eax & 0x1f == 0 {
            break;
        }
        if (cache.eax >> 5) & 0x7 < 3 {
            continue;
        }
        if let Some(bytes) = cache_bytes(cache.ebx, cache.ecx) {
            largest = largest.max(bytes);
        }
    }
    (largest > 0).then_some(largest)
}

/// Bytes a cache descriptor stands for, or nothing where the four fields are not believable
fn cache_bytes(ebx: u32, ecx: u32) -> Option<usize> {
    let ways = ((ebx >> 22) & 0x3ff) as usize + 1;
    let partitions = ((ebx >> 12) & 0x3ff) as usize + 1;
    let line = (ebx & 0xfff) as usize + 1;
    let sets = ecx as usize + 1;
    ways.checked_mul(partitions)
        .and_then(|size| size.checked_mul(line))
        .and_then(|size| size.checked_mul(sets))
        .filter(|size| (SMALLEST_CACHE..=LARGEST_CACHE).contains(size))
}

/// Pin the streaming floor for a test, zero putting it back to what the processor says
#[cfg(feature = "internals")]
pub fn force_stream_floor(bytes: usize) {
    FLOOR.store(bytes, Ordering::Relaxed);
}

/// Pin the path, for a test that wants one the machine would not choose
///
/// # Safety
///
/// The path must be one the running processor can execute.
#[cfg(feature = "internals")]
pub unsafe fn force(chosen: u8) {
    CHOSEN.store(chosen, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    // a descriptor of all ones is refused rather than multiplied out
    #[test]
    fn absurd_geometry_is_refused() {
        assert_eq!(cache_bytes(u32::MAX, u32::MAX), None);
        assert_eq!(cache_bytes(0, 0), None);
        assert_eq!(cache_bytes(u32::MAX, 0), None);
    }

    // 16 ways, one partition, 64 byte lines, 32768 sets: a 32 MiB last level
    #[test]
    fn a_real_descriptor_measures_its_cache() {
        assert_eq!(cache_bytes((15 << 22) | 63, 32_767), Some(32 << 20));
    }

    // a believable cache leaves a floor nothing mistakes for `UNMEASURED`
    #[test]
    fn a_measured_floor_is_not_the_sentinel() {
        assert_ne!(
            cache_bytes((15 << 22) | 63, 32_767).unwrap() / 7 * 3,
            UNMEASURED
        );
    }
}
