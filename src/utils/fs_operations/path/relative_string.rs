use memchr::{memchr, memrchr};
use smallvec::SmallVec;
use std::borrow::Cow;

type StrVec<'a> = SmallVec<[&'a str; 8]>;

pub(super) fn relative_str<'a>(target: &'a str, base: &str) -> Cow<'a, str> {
    let target = target.trim_end_matches('/');
    let base = base.trim_end_matches('/');
    if needs_relative_normalization(target) || needs_relative_normalization(base) {
        Cow::Owned(relative_str_slow(target, base))
    } else {
        relative_str_fast(target, base)
    }
}

#[cfg(not(target_family = "windows"))]
#[inline]
fn common_prefix_len_case_sensitive(left: &[u8], right: &[u8]) -> usize {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        common_prefix_len_neon(left, right)
    }
    #[cfg(not(all(target_arch = "aarch64", target_feature = "neon")))]
    {
        common_prefix_len_scalar(left, right)
    }
}

#[cfg(not(target_family = "windows"))]
#[inline]
fn common_prefix_len_scalar(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .position(|(left, right)| left != right)
        .unwrap_or(left.len().min(right.len()))
}

#[cfg(all(
    not(target_family = "windows"),
    target_arch = "aarch64",
    target_feature = "neon"
))]
#[inline]
fn common_prefix_len_neon(left: &[u8], right: &[u8]) -> usize {
    use std::arch::aarch64::{vceqq_u8, vld1q_u8, vminvq_u8, vst1q_u8};

    let len = left.len().min(right.len());
    let vector_end = len & !15;
    let mut offset = 0;

    while offset < vector_end {
        // SAFETY: `vector_end` is `len` rounded down to a multiple of 16, and
        // `offset` advances by exactly 16. Both loads therefore stay within the
        // shorter input, including when either slice starts at an unaligned address.
        let equal = unsafe {
            let left_chunk = vld1q_u8(left.as_ptr().add(offset));
            let right_chunk = vld1q_u8(right.as_ptr().add(offset));
            vceqq_u8(left_chunk, right_chunk)
        };
        // Equality lanes are all ones for a match and all zeroes for a mismatch.
        if unsafe { vminvq_u8(equal) } != u8::MAX {
            let mut equal_bytes = [0; 16];
            // SAFETY: `equal_bytes` has exactly enough initialized space for one
            // 16-byte vector store.
            unsafe { vst1q_u8(equal_bytes.as_mut_ptr(), equal) };
            let mismatch_bits = !u128::from_le_bytes(equal_bytes);
            debug_assert_ne!(mismatch_bits, 0);
            return offset + mismatch_bits.trailing_zeros() as usize / 8;
        }
        offset += 16;
    }

    offset + common_prefix_len_scalar(&left[offset..len], &right[offset..len])
}

/// Check if a path contains components or separators that need normalization.
/// Uses `memchr` to jump between `/` positions — most bytes in a path aren't `/`,
/// so this skips the vast majority of the input.
#[inline]
fn needs_relative_normalization(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() > 1 && bytes.last() == Some(&b'/') {
        return true;
    }
    if bytes.first() == Some(&b'.') {
        if bytes.len() == 1 || bytes.get(1) == Some(&b'/') {
            return true;
        }
        if bytes.get(1) == Some(&b'.') && (bytes.len() == 2 || bytes.get(2) == Some(&b'/')) {
            return true;
        }
    }
    let mut offset = 0;
    while let Some(pos) = memchr(b'/', &bytes[offset..]) {
        let slash = offset + pos;
        if slash + 1 < bytes.len() && bytes[slash + 1] == b'/' {
            return true;
        }
        if slash + 1 < bytes.len() && bytes[slash + 1] == b'.' {
            let after_dot = slash + 2;
            // "/." at end or "/./"
            if after_dot >= bytes.len() || bytes[after_dot] == b'/' {
                return true;
            }
            // "/.." at end or "/../"
            if bytes[after_dot] == b'.'
                && (after_dot + 1 >= bytes.len() || bytes[after_dot + 1] == b'/')
            {
                return true;
            }
        }
        offset = slash + 1;
    }
    false
}

/// Fast path: no normalization needed. Operates directly on `&str` slices
/// with zero intermediate allocation.
fn relative_str_fast<'a>(target: &'a str, base: &str) -> Cow<'a, str> {
    let common_byte_len = {
        #[cfg(target_family = "windows")]
        {
            target
                .as_bytes()
                .iter()
                .zip(base.as_bytes().iter())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count()
        }
        #[cfg(not(target_family = "windows"))]
        {
            common_prefix_len_case_sensitive(target.as_bytes(), base.as_bytes())
        }
    };

    // Adjust to last '/' boundary to ensure we match full path components
    // Check if common_byte_len falls on a component boundary:
    // - exact match (both exhausted)
    // - one side exhausted and the other has '/' next (prefix match)
    let at_boundary = (common_byte_len == target.len() && common_byte_len == base.len())
        || (common_byte_len == target.len() && base.as_bytes().get(common_byte_len) == Some(&b'/'))
        || (common_byte_len == base.len() && target.as_bytes().get(common_byte_len) == Some(&b'/'));
    let common_prefix = if at_boundary {
        common_byte_len
    } else {
        memrchr(b'/', &target.as_bytes()[..common_byte_len]).unwrap_or(0)
    };

    // Count remaining base components
    let base_remaining = &base.as_bytes()[common_prefix..];
    let mut ups = 0u32;
    {
        let mut offset = 0;
        while offset < base_remaining.len() {
            if base_remaining[offset] == b'/' {
                offset += 1;
                continue;
            }
            ups += 1;
            offset = match memchr(b'/', &base_remaining[offset..]) {
                Some(pos) => offset + pos + 1,
                None => base_remaining.len(),
            };
        }
    }

    let target_suffix = target[common_prefix..].trim_start_matches('/');
    let ups = ups as usize;
    if ups == 0 {
        return Cow::Borrowed(target_suffix);
    }
    let suffix_iter = if target_suffix.is_empty() {
        None
    } else {
        Some(target_suffix)
    };
    let mut result = String::with_capacity(ups * 3 + target_suffix.len());
    std::iter::repeat_n("..", ups)
        .chain(suffix_iter)
        .for_each(|s| {
            if !result.is_empty() {
                result.push('/');
            }
            result.push_str(s);
        });
    Cow::Owned(result)
}

/// Slow path: normalize `.` and `..` components first, then compute relative path.
fn relative_str_slow(target: &str, base: &str) -> String {
    let target_parts = normalize_parts(target);
    let base_parts = normalize_parts(base);

    let common_len = {
        #[cfg(target_family = "windows")]
        {
            target_parts
                .iter()
                .zip(base_parts.iter())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count()
        }
        #[cfg(not(target_family = "windows"))]
        {
            target_parts
                .iter()
                .zip(base_parts.iter())
                .take_while(|(a, b)| a == b)
                .count()
        }
    };

    let ups = base_parts.len() - common_len;
    let remaining = &target_parts[common_len..];

    let remaining_len: usize =
        remaining.iter().map(|s| s.len()).sum::<usize>() + remaining.len().saturating_sub(1);
    let mut result = String::with_capacity(ups * 3 + remaining_len);
    std::iter::repeat_n("..", ups)
        .chain(remaining.iter().copied())
        .for_each(|s| {
            if !result.is_empty() {
                result.push('/');
            }
            result.push_str(s);
        });
    result
}

/// Split a path into normalized components, resolving `.` and `..` lexically.
fn normalize_parts(path: &str) -> StrVec<'_> {
    let mut parts = StrVec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts
}

#[cfg(all(
    test,
    not(target_family = "windows"),
    target_arch = "aarch64",
    target_feature = "neon"
))]
mod common_prefix_tests {
    use super::{common_prefix_len_neon, common_prefix_len_scalar};

    fn bytes(len: usize) -> Vec<u8> {
        (0..len)
            .map(|index| ((index * 37 + 11) % 251) as u8)
            .collect()
    }

    #[test]
    fn neon_matches_scalar_for_lengths_and_every_mismatch() {
        for len in 0..=256 {
            let left = bytes(len);
            assert_eq!(
                common_prefix_len_neon(&left, &left),
                len,
                "equal input length {len}"
            );

            for mismatch in 0..len {
                let mut right = left.clone();
                right[mismatch] ^= u8::MAX;
                assert_eq!(
                    common_prefix_len_neon(&left, &right),
                    common_prefix_len_scalar(&left, &right),
                    "input length {len}, mismatch {mismatch}",
                );
            }
        }
    }

    #[test]
    fn neon_matches_scalar_for_every_length_pair() {
        let input = bytes(256);
        for left_len in 0..=256 {
            for right_len in 0..=256 {
                assert_eq!(
                    common_prefix_len_neon(&input[..left_len], &input[..right_len]),
                    common_prefix_len_scalar(&input[..left_len], &input[..right_len]),
                    "left length {left_len}, right length {right_len}",
                );
            }
        }
    }

    #[test]
    fn neon_handles_all_sixteen_byte_alignment_pairs() {
        for left_offset in 0..16 {
            for right_offset in 0..16 {
                for len in 0..=256 {
                    let mut left = [0xa5; 288];
                    let mut right = [0x5a; 288];
                    for index in 0..len {
                        let value = ((index * 37 + 11) % 251) as u8;
                        left[left_offset + index] = value;
                        right[right_offset + index] = value;
                    }
                    let left = &left[left_offset..left_offset + len];
                    let right = &right[right_offset..right_offset + len];
                    assert_eq!(
                        common_prefix_len_neon(left, right),
                        len,
                        "left offset {left_offset}, right offset {right_offset}, length {len}",
                    );
                }
            }
        }
    }
}
